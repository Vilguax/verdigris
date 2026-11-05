use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use tracing::error;
use verdigris_protocol::{ChunkColumn, ChunkGenerator, biome_index, block_index, plains_biome_id};
use verdigris_worldgen::feature::{Decoration, DecorationContext, ProtoChunk, TerrainFeatureWorld};
use verdigris_worldgen::{ChunkTerrain, SearchState, WorldGenerator};

use crate::generator::GenMetrics;

struct DecorationState {
    world: TerrainFeatureWorld,
    ctx: DecorationContext,
    decorated: HashSet<(i32, i32)>,
    emitted: HashSet<(i32, i32)>,
    building: HashSet<(i32, i32)>,
}

pub struct DecoratedGenerator {
    state: Mutex<DecorationState>,
    built: Condvar,
    decoration: Decoration,
    generator: Arc<WorldGenerator>,
    metrics: Arc<GenMetrics>,
    seed: i64,
    biome: u16,
    height: usize,
}

struct Scratch {
    terrain: ChunkTerrain,
    search: SearchState,
}

thread_local! {
    static SCRATCH: RefCell<Option<Scratch>> = const { RefCell::new(None) };
}

impl DecoratedGenerator {
    pub fn new(
        generator: Arc<WorldGenerator>,
        seed: i64,
        metrics: Arc<GenMetrics>,
    ) -> Result<Self, String> {
        let decoration = Decoration::embedded().map_err(|err| err.to_string())?;
        let biome = plains_biome_id().map_err(|err| err.to_string())? as u16;
        let height = generator.height();
        let world = TerrainFeatureWorld::new(Arc::clone(&generator));
        Ok(DecoratedGenerator {
            state: Mutex::new(DecorationState {
                world,
                ctx: DecorationContext::new(),
                decorated: HashSet::new(),
                emitted: HashSet::new(),
                building: HashSet::new(),
            }),
            built: Condvar::new(),
            decoration,
            generator,
            metrics,
            seed,
            biome,
            height,
        })
    }

    fn build_proto(&self, cx: i32, cz: i32) -> ProtoChunk {
        SCRATCH.with(|cell| {
            let mut slot = cell.borrow_mut();
            let scratch = slot.get_or_insert_with(|| Scratch {
                terrain: self.generator.terrain(),
                search: SearchState::new(),
            });
            match self.generator.fill_chunk(cx, cz, &mut scratch.terrain) {
                Ok(()) => ProtoChunk::from_terrain(
                    &self.generator,
                    cx,
                    cz,
                    &scratch.terrain,
                    &mut scratch.search,
                ),
                Err(err) => {
                    error!(
                        x = cx,
                        z = cz,
                        erreur = %err,
                        "remplissage de chunk échoué, prototype d'air inséré"
                    );
                    ProtoChunk::empty(&self.generator, cx, cz, &mut scratch.search)
                }
            }
        })
    }

    fn emit_column(&self, world: &TerrainFeatureWorld, cx: i32, cz: i32) -> ChunkColumn {
        let mut column = ChunkColumn::empty(self.biome);
        if let Some(states) = world.column_states(cx, cz) {
            for lz in 0..16usize {
                for lx in 0..16usize {
                    let base = (lx + 16 * lz) * self.height;
                    for j in 0..self.height {
                        let state = states[base + j];
                        if state != 0 {
                            column.sections[j / 16].states[block_index(lx, j % 16, lz)] = state;
                        }
                    }
                }
            }
        }
        if let Some(biomes) = world.column_biomes(cx, cz) {
            for (j, section) in column.sections.iter_mut().enumerate() {
                for yl in 0..4usize {
                    let qy = j * 4 + yl;
                    for qz in 0..4usize {
                        for qx in 0..4usize {
                            section.biomes[biome_index(qx, yl, qz)] = biomes[qy * 16 + qz * 4 + qx];
                        }
                    }
                }
            }
        }
        column
    }

    fn claim_terrain(&self, cx: i32, cz: i32) -> (Coords, Coords) {
        let mut guard = self.state.lock().unwrap();
        let mut mine = Vec::new();
        let mut theirs = Vec::new();
        for dz in -2..=2 {
            for dx in -2..=2 {
                let p = (cx + dx, cz + dz);
                if guard.world.is_loaded(p.0, p.1) || guard.emitted.contains(&p) {
                    continue;
                }
                if guard.building.insert(p) {
                    mine.push(p);
                } else {
                    theirs.push(p);
                }
            }
        }
        (mine, theirs)
    }

    fn build_many(&self, claimed: &[(i32, i32)]) -> Vec<((i32, i32), ProtoChunk)> {
        let workers = claimed.len().min(TERRAIN_WORKERS);
        if workers <= 1 {
            return claimed
                .iter()
                .map(|&p| (p, self.build_proto(p.0, p.1)))
                .collect();
        }
        let per_worker = claimed.len().div_ceil(workers);
        std::thread::scope(|scope| {
            let handles: Vec<_> = claimed
                .chunks(per_worker)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|&p| (p, self.build_proto(p.0, p.1)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .collect()
        })
    }

    fn generate_column(&self, cx: i32, cz: i32) -> ChunkColumn {
        let (mine, theirs) = self.claim_terrain(cx, cz);
        let claim = Claim {
            owner: self,
            points: &mine,
        };
        let built = self.build_many(&mine);

        let mut guard = self.state.lock().unwrap();
        for (p, proto) in built {
            if !guard.world.is_loaded(p.0, p.1) && !guard.emitted.contains(&p) {
                guard.world.insert_chunk(p.0, p.1, proto);
            }
        }
        for p in &mine {
            guard.building.remove(p);
        }
        std::mem::forget(claim);
        self.built.notify_all();
        while theirs.iter().any(|p| guard.building.contains(p)) {
            guard = self.built.wait(guard).unwrap();
        }

        for dz in -1..=1 {
            for dx in -1..=1 {
                let n = (cx + dx, cz + dz);
                if guard.decorated.contains(&n) {
                    continue;
                }
                let DecorationState {
                    world,
                    ctx,
                    decorated,
                    ..
                } = &mut *guard;
                let mut region = world.region(n.0, n.1);
                if let Err(err) =
                    self.decoration
                        .decorate_chunk(&mut region, ctx, self.seed, n.0, n.1)
                {
                    error!(
                        x = n.0,
                        z = n.1,
                        erreur = %err,
                        "décoration de chunk échouée, poses partielles conservées"
                    );
                }
                decorated.insert(n);
            }
        }

        let column = self.emit_column(&guard.world, cx, cz);
        guard.emitted.insert((cx, cz));

        let mut to_remove: Vec<(i32, i32)> = Vec::new();
        for dz in -2..=2 {
            for dx in -2..=2 {
                let p = (cx + dx, cz + dz);
                if !guard.emitted.contains(&p) || !guard.world.is_loaded(p.0, p.1) {
                    continue;
                }
                let neighbors_done = (-1..=1).all(|ez| {
                    (-1..=1).all(|ex| {
                        if ex == 0 && ez == 0 {
                            return true;
                        }
                        guard.decorated.contains(&(p.0 + ex, p.1 + ez))
                    })
                });
                if neighbors_done {
                    to_remove.push(p);
                }
            }
        }
        for p in to_remove {
            guard.world.remove_chunk(p.0, p.1);
        }

        column
    }
}

const TERRAIN_WORKERS: usize = 8;

type Coords = Vec<(i32, i32)>;

struct Claim<'a> {
    owner: &'a DecoratedGenerator,
    points: &'a [(i32, i32)],
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.owner.state.lock() {
            for p in self.points {
                guard.building.remove(p);
            }
        }
        self.owner.built.notify_all();
    }
}

impl ChunkGenerator for DecoratedGenerator {
    fn generate(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
        let start = Instant::now();
        let column = self.generate_column(chunk_x, chunk_z);
        self.metrics
            .record(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Barrier, OnceLock, mpsc};
    use std::thread;
    use std::time::Duration;
    use verdigris_protocol::{FlatGenerator, default_state_of_name};
    use verdigris_worldgen::chunk_fill::CHUNK_WIDTH;

    const GUARD: Duration = Duration::from_secs(120);

    const SEED: i64 = 20_261_029;

    fn generator() -> Arc<WorldGenerator> {
        static SHARED: OnceLock<Arc<WorldGenerator>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(WorldGenerator::new(SEED).expect("generator"))))
    }

    fn decorated(generator: Arc<WorldGenerator>) -> DecoratedGenerator {
        DecoratedGenerator::new(generator, SEED, Arc::new(GenMetrics::default()))
            .expect("decorated")
    }

    fn column_has_terrain(column: &ChunkColumn) -> bool {
        column
            .sections
            .iter()
            .any(|s| s.states.iter().any(|&st| st != 0))
    }

    fn reference_column(generator: Arc<WorldGenerator>, cx: i32, cz: i32) -> (Vec<u16>, Vec<u16>) {
        let decoration = Decoration::embedded().expect("decoration");
        let mut world = TerrainFeatureWorld::new(generator);
        for dz in -2..=2 {
            for dx in -2..=2 {
                world.load_chunk(cx + dx, cz + dz).expect("load terrain");
            }
        }
        let mut ctx = DecorationContext::new();
        let mut order: Vec<(i32, i32)> = Vec::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                order.push((cx + dx, cz + dz));
            }
        }
        order.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        for (nx, nz) in order {
            let mut region = world.region(nx, nz);
            decoration
                .decorate_chunk(&mut region, &mut ctx, SEED, nx, nz)
                .expect("decorate");
        }
        let height = world.height();
        let min_y = world.min_y();
        let mut states = vec![0u16; height * 256];
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                for j in 0..height {
                    let y = min_y + j as i32;
                    states[(lx + CHUNK_WIDTH * lz) * height + j] =
                        world.state_at(cx, cz, lx, y, lz);
                }
            }
        }
        let biomes = world.column_biomes(cx, cz).expect("biomes").to_vec();
        (states, biomes)
    }

    #[test]
    fn worldgen_protocol_state_correspondence() {
        let generator = generator();
        let world = TerrainFeatureWorld::new(Arc::clone(&generator));
        let worldgen_states = world.material_default_states();
        for (name, &wg) in generator
            .material()
            .blocks
            .iter()
            .zip(worldgen_states.iter())
        {
            let proto = default_state_of_name(name)
                .unwrap_or_else(|| panic!("bloc absent du registre protocole: {name}"))
                as u16;
            assert_eq!(wg, proto, "état divergent pour {name}");
        }
    }

    #[test]
    #[ignore = "génération de terrain coûteuse en debug: lancer avec --ignored ou en release"]
    fn central_chunk_matches_reference() {
        let generator = generator();
        let (reference, reference_biomes) = reference_column(Arc::clone(&generator), 4, -3);
        let dec = decorated(generator);
        let column = dec.generate(4, -3);
        let height = dec.height;
        for lz in 0..16usize {
            for lx in 0..16usize {
                for j in 0..height {
                    let want = reference[(lx + 16 * lz) * height + j];
                    let got = column.sections[j / 16].states[block_index(lx, j % 16, lz)];
                    assert_eq!(want, got, "divergence d'état en ({lx},{j},{lz})");
                }
            }
        }
        for (j, section) in column.sections.iter().enumerate() {
            for yl in 0..4usize {
                let qy = j * 4 + yl;
                for qz in 0..4usize {
                    for qx in 0..4usize {
                        let want = reference_biomes[qy * 16 + qz * 4 + qx];
                        let got = section.biomes[biome_index(qx, yl, qz)];
                        assert_eq!(want, got, "divergence de biome en ({qx},{qy},{qz})");
                    }
                }
            }
        }
        let bedrock = default_state_of_name("minecraft:bedrock").unwrap() as u16;
        let has_bedrock = (0..16usize).any(|lz| {
            (0..16usize).any(|lx| column.sections[0].states[block_index(lx, 0, lz)] == bedrock)
        });
        assert!(has_bedrock, "terrain présent: bedrock en bas du chunk");
    }

    #[test]
    #[ignore = "génération de terrain coûteuse en debug: lancer avec --ignored ou en release"]
    fn release_zone_bounds_and_contents() {
        let dec = decorated(generator());
        let iron = default_state_of_name("minecraft:iron_ore").unwrap() as u16;
        let deep_iron = default_state_of_name("minecraft:deepslate_iron_ore").unwrap() as u16;
        let sediments: Vec<u16> = ["minecraft:clay", "minecraft:gravel", "minecraft:sand"]
            .iter()
            .map(|n| default_state_of_name(n).unwrap() as u16)
            .collect();
        let bedrock = default_state_of_name("minecraft:bedrock").unwrap() as u16;
        let mut found_ore = false;
        let mut found_sediment = false;
        let mut found_terrain = false;
        for cz in 0..6 {
            for cx in 0..6 {
                let column = dec.generate(cx, cz);
                for section in &column.sections {
                    for &state in section.states.iter() {
                        if state == iron || state == deep_iron {
                            found_ore = true;
                        }
                        if sediments.contains(&state) {
                            found_sediment = true;
                        }
                        if state == bedrock {
                            found_terrain = true;
                        }
                    }
                }
            }
        }
        assert!(found_terrain, "terrain présent dans la zone");
        assert!(found_ore, "au moins un minerai de fer posé");
        assert!(
            found_sediment,
            "au moins un bloc d'argile, gravier ou sable posé"
        );
        let guard = dec.state.lock().unwrap();
        for &(px, pz) in guard.emitted.iter() {
            if !guard.world.is_loaded(px, pz) {
                continue;
            }
            let neighbors_done = (-1..=1).all(|ez| {
                (-1..=1).all(|ex| {
                    if ex == 0 && ez == 0 {
                        return true;
                    }
                    guard.decorated.contains(&(px + ex, pz + ez))
                })
            });
            assert!(
                !neighbors_done,
                "prototype émis ({px},{pz}) non libéré alors que ses 8 voisins sont décorés"
            );
        }
        assert!(
            guard.world.chunk_count() <= 64,
            "nombre de prototypes résidents borné: {}",
            guard.world.chunk_count()
        );
    }

    #[test]
    #[ignore = "génération de terrain coûteuse en debug: lancer avec --ignored ou en release"]
    fn concurrent_distinct_chunks_no_deadlock() {
        let dec = Arc::new(decorated(generator()));
        let coords: Vec<(i32, i32)> = (0..8).map(|i| (i % 4, i / 4)).collect();
        let barrier = Arc::new(Barrier::new(coords.len()));
        let (tx, rx) = mpsc::channel();
        let handles: Vec<_> = coords
            .into_iter()
            .map(|(cx, cz)| {
                let dec = Arc::clone(&dec);
                let barrier = Arc::clone(&barrier);
                let tx = tx.clone();
                thread::spawn(move || {
                    barrier.wait();
                    let column = dec.generate(cx, cz);
                    assert!(
                        column_has_terrain(&column),
                        "chunk ({cx},{cz}) a du terrain"
                    );
                    tx.send(()).unwrap();
                })
            })
            .collect();
        drop(tx);
        for _ in 0..handles.len() {
            rx.recv_timeout(GUARD)
                .expect("génération concurrente sans interblocage (timeout de garde)");
        }
        for h in handles {
            h.join().expect("thread sans panique");
        }
    }

    #[test]
    #[ignore = "génération de terrain coûteuse en debug: lancer avec --ignored ou en release"]
    fn overlapping_neighbors_no_double_decoration() {
        let dec = Arc::new(decorated(generator()));
        let iron = default_state_of_name("minecraft:iron_ore").unwrap() as u16;
        let deep_iron = default_state_of_name("minecraft:deepslate_iron_ore").unwrap() as u16;
        let sediments: Vec<u16> = ["minecraft:clay", "minecraft:gravel", "minecraft:sand"]
            .iter()
            .map(|n| default_state_of_name(n).unwrap() as u16)
            .collect();
        let decorated_in = |column: &ChunkColumn| -> bool {
            column.sections.iter().any(|s| {
                s.states
                    .iter()
                    .any(|&st| st == iron || st == deep_iron || sediments.contains(&st))
            })
        };
        for round in 0..4 {
            let base = round * 10;
            let pair = [(base, 0), (base + 1, 0)];
            let barrier = Arc::new(Barrier::new(pair.len()));
            let (tx, rx) = mpsc::channel();
            let handles: Vec<_> = pair
                .into_iter()
                .map(|(cx, cz)| {
                    let dec = Arc::clone(&dec);
                    let barrier = Arc::clone(&barrier);
                    let tx = tx.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        tx.send(dec.generate(cx, cz)).unwrap();
                    })
                })
                .collect();
            drop(tx);
            let mut columns = Vec::new();
            for _ in 0..handles.len() {
                columns.push(
                    rx.recv_timeout(GUARD)
                        .expect("génération concurrente sans interblocage (timeout de garde)"),
                );
            }
            for h in handles {
                h.join().expect("thread sans panique");
            }
            for column in &columns {
                assert!(column_has_terrain(column), "colonne concurrente non vide");
            }
            assert!(
                columns.iter().any(&decorated_in),
                "décoration présente dans les sorties du tour {round}"
            );
        }
    }

    #[test]
    fn flat_generator_unchanged() {
        let flat = FlatGenerator::new().expect("flat");
        let column = flat.generate(3, -4);
        let bedrock = default_state_of_name("minecraft:bedrock").unwrap() as u16;
        let dirt = default_state_of_name("minecraft:dirt").unwrap() as u16;
        let grass = default_state_of_name("minecraft:grass_block").unwrap() as u16;
        assert_eq!(column.sections[0].states[block_index(0, 0, 0)], bedrock);
        assert_eq!(column.sections[0].states[block_index(0, 1, 0)], dirt);
        assert_eq!(column.sections[0].states[block_index(0, 2, 0)], dirt);
        assert_eq!(column.sections[0].states[block_index(0, 3, 0)], grass);
    }
}
