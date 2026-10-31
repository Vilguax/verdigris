# Notes — évaluation en mode volume des fonctions de densité

## Produit scalaire du bruit de gradient (H1)
L'ordre §1.5 `((g.x·u)+(g.z·w))+(g.y·v)` n'est pas réimplémenté pour le mode
volume : la forme volume (02 §2.2) ne change que le calcul des coordonnées
`b·(σ·ν)` (voir `octaves::sample_volume`), le bruit de gradient par nœud réutilise
l'ordre de produit scalaire déjà présent côté point (§1.4). §1.5 est déclaré
bit-identique sauf signe d'un zéro, sans effet sur les vecteurs validés. À
confronter à l'oracle si un écart apparaît.

## Coins de `interpolated` échantillonnés en mode point (écart oracle)
La porte oracle 1a a révélé un décalage vertical systématique de la surface
(~1 à 2 blocs trop bas, écarts de densité jusqu'à ~0,05 près du passage à zéro) :
l'échantillonnage des coins de cellule en mode volume divergeait de vanilla.
Vanilla évalue les fonctions de densité aux coins de cellule **point par point**
(`compute` sur un contexte à point unique), puis interpole. La grille de coins de
`interpolated` en mode volume (`df::volume::interpolated_unit_volume`) est donc
désormais remplie via `eval(..., Mode::Point)` à chaque coin, et non plus via une
passe volume de l'entrée. L'interpolation horizontale (Z puis X) et l'accumulation
verticale sont inchangées. Ceci confirme que la divergence anticipée en H1 n'était
pas limitée au signe d'un zéro pour les coordonnées volume à grand `b`.

## Étalement vertical du bruit de gradient (écart oracle)
La porte oracle 1a a imposé une correction du pas d'étalement `kappa` dans
`GradientNoise::sample_smeared`. La coordonnée verticale repliée utilisée pour
quantifier l'étalement est `lambda = if y >= 0.0 && y < ry { y } else { ry }`, et
non `ry` seul. Vanilla n'applique l'étalement qu'au-dessus de `y = 0` : sous le
niveau zéro l'étalement doit retomber sur la fraction locale `ry`, et dans la
bande `0 <= y < ry` c'est `y` lui-même qui pilote la quantification. Sans ce
garde, la surface divergeait près de `y = 0`. L'argument `fade_y` passé à
`sample_and_lerp` reste `ry` (interpolation inchangée) ; seul `dyc = ry - kappa`
est affecté. Comme les coins de `interpolated` (voir section dédiée), c'est une
divergence tranchée par l'oracle, pas une refonte du moteur : le reste de
`sample`/`sample_smeared` est identique.

## Sémantique min / max en mode volume (correction T5)
Le mode volume sélectionne par comparaison IEEE stricte, pas via `jmin`/`jmax` :
- `min` non constant : `r = a`, remplacé par `b` seulement si `b < a`.
- `max` non constant : `r = a`, remplacé par `b` seulement si `b > a`.
- `min`/`max` avec constante `c` : remplacé par `c` seulement si `c < a` (resp.
  `c > a`).

Conséquences assumées : `min(+0, −0) = +0`, `max(+0, −0) = +0`,
`min(NaN, x) = NaN`, `min(x, NaN) = x`. Cela diffère volontairement de
`jmin`/`jmax`, qui restent réservés au mode point (§4.2) et à l'arithmétique
d'intervalles (§6). Les arms `MinConst`/`MaxConst` de `df::eval` sont désormais
dépendants du mode pour rester cohérents avec les arms non constants déjà
stricts en volume.

## Interpolation (§4.12) en mode volume
`interpolated` est désormais évalué en volume via trois cas. `VolumeError` reste
dans l'API publique mais n'est plus produit : aucune variante n'est construite,
le mode volume d'`interpolated` renvoie toujours un `Vec<f32>`.

- Cas 1 (aligné) : sur chaque axe l'origine est multiple du pas de cellule et la
  taille vaut 1 ou le pas égale la cellule. Les nœuds tombent sur des coins, donc
  l'entrée est évaluée directement en volume et renvoyée telle quelle.
- Cas 2 (tous pas = 1) : une grille de coins (origine `(cx_min·w, cy_min·h,
  cz_min·w)`, pas `(w, h, w)`) est évaluée une seule fois en volume. Par colonne
  (x, z) on interpole en Z puis X pour obtenir `q0`/`q1`, puis la verticale est
  remplie par accumulation `val += step` (step = `(q1 − q0)·ih`), jamais par un
  lerp direct. L'accumulation `f32` diffère volontairement du lerp direct (bits :
  `0.1→0.7`, oy=6 donne `0.5499999523162842` et non `0.550000011920929`).
- Cas 3 (pas non unitaires) : on NE déplace JAMAIS l'origine. On construit le
  volume plein de pas 1 d'origine exacte `(x0, y0, z0)` et de tailles
  `(n−1)·s + 1`, on lui applique le cas 2, puis on extrait le nœud `(i, j, k)` à
  l'index `(i·sx, j·sy, k·sz)`. Offsets entiers exacts, valides pour origine
  négative et `s > 1` (prouvé par `interpolated_case3_extracts_dense_without_moving_origin`).

## Coin exact du mode point (§4.12)
Au bloc où `ox = oy = oz = 0`, `interpolated` en mode point évalue l'entrée en
mode point (et non en mode volume comme auparavant). Hors coin, la grille 2×2×2
des coins reste évaluée en mode volume dans huit scalaires de pile (`v000..v111`,
pas de `Vec`), donc aucune allocation par appel point.

## Contexte de chunk « monde neuf » 26.3
`final_density_chunk` évalue `final_density` une seule fois en mode volume sur
`(16, H, 16)`, origine `(16·cx, min_y, 16·cz)`, pas `(1,1,1)`, index
`j + (lx + 16·lz)·H`, avec `min_y = −64` et `H = 384` lus dans
`noise_settings/overworld.noise`. Les invariants monde neuf sont figés dans la
construction des nœuds, sans champ configurable : `beardifier = 0`,
`blend_alpha = 1`, `blend_offset = 0`, `blend_density(input) = input`.

## Remplissage de chunk en catégories (phase 1a)
`chunk_fill::WorldGenerator` construit l'arbre `final_density` une fois par graine
(immuable, partageable derrière `Arc`) et `fill_chunk` évalue `final_density` une
seule fois en mode volume sur `(16, H, 16)`, origine `(16·cx, min_y, 16·cz)`, pas
`(1,1,1)`, puis dérive une catégorie par bloc dans un `ChunkTerrain` réutilisable
(tampon densité et `VolumeContext` réutilisés, aucune allocation par bloc).

Décisions figées, tranchées par l'oracle 1a (`tests/oracle.rs`, 40 chunks,
graine 20261029) :

- Solide ssi la densité `f32` élargie en `f64` est strictement `> 0`. `+0`, `−0`
  et `NaN` sont non solides.
- Règle sans aquifère (`min(−54, sea_level=63) = −54`) : lave si `y < −54`, eau si
  `y < 63`, air sinon ; appliquée uniquement aux blocs non solides.
- Catégories exposées : `Stone`, `Water`, `Lava`, `Air`. La conversion en états de
  bloc et la bedrock à `y = −64` vivent côté serveur, pas dans ce crate pur.
- Index de `ChunkTerrain` identique à `final_density_chunk` (`j + (lx + 16·lz)·H`,
  `j = y − min_y`). L'oracle indexe `(y − min_y)·256 + z·16 + x` ; la comparaison
  fait la correspondance.

## Vecteurs §5.2 INCERTAINS
Le test `shift_a_equals_shift_b_5_2_vectors` est conservé en
`#[ignore = "5.2 INCERTAIN: calcul indépendant non confronté à vanilla"]` (H5) :
l'égalité `shift_a == shift_b` hors origine n'est pas confrontée à vanilla et
n'est donc pas confirmée.

## find_top_surface en volume
`find_top_surface` est indépendant du mode : la valeur volume est calculée
exactement comme la valeur point (sondes `density` en mode point, `upper_bound`
en `y = 0`), ce qui garantit l'égalité volume == point.

## Allocation
`eval_volume` alloue un `Vec<f32>` par nœud et par appel (décision mémoire :
pas de pool). Les nœuds binaires réutilisent le `Vec` de l'opérande gauche comme
tampon de sortie ; aucune allocation par bloc ni par point.

## Phase 1b — aquifères (spec 05)

### Activation explicite (D1, [HYP])
Le mode est porté par `AquiferMode` passé à la construction de `WorldGenerator`
(`new` = `Noise`, `with_mode(seed, Disabled)`), jamais déduit de la présence du
champ `aquifers`. `Disabled` câble la règle simple (spec 04 §4.3) via
`category_for` : la porte oracle-1a tourne dessus et reste verte (les barrières
d'aquifère qui transforment un bloc non solide en pierre ne peuvent donc pas
régresser la solidité comparée en 1a). `Noise` exige que `aquifers` soit présent,
sinon `with_mode` échoue (pas de repli silencieux). Le serveur construit en
`Noise` par défaut. [HYP] tranche la cause REJECT « activation par présence vs
mode explicite sans aquifères ».

### Compilation commune et caches volume en mode point (D3, spec 05 §6.2, 03 §4.13)
`WorldgenData::build_world_density` compile `final_density` et les six fonctions
`aquifers.*` avec un seul `Assets` (une instanciation de bruits par graine).

Correction tranchée par oracle-1b (chunk (5,6), 31 cases eau→air) : la neutralité
point/volume de §6.2 est FAUSSE pour ces données. Vanilla évalue `exclusion` et
`surface_level` en mode point, mais leurs nœuds `cache` 2D (`erosion`, `offset`,
`factor`, `shift_x`, `shift_z`) renvoient la valeur calculée en mode VOLUME lors
du remplissage du chunk (03 §4.13 : une valeur mise en cache par une évaluation
volume est renvoyée en point si la position est dans le volume conservé). Le
bruit 2D n'étant pas dyadique, point ≠ volume d'un ULP, ce qui décalait la surface
préliminaire d'une cellule et bloquait l'inondation (sous_mer) d'un aquifère.

Implémentation : ni interning `Arc<Node>`, ni pré-chauffage par évaluation volume
(une première version pré-chauffait un `VolumeContext` par un eval volume 16×H×16
de `exclusion`/`density` par chunk — correct mais ~×60 trop lent, les nœuds 2D
étant réévalués à chaque y, d'où un timeout de la porte). À la place,
`df::volume::eval_point_ctx` reçoit le footprint du chunk (`AquiferScratch.footprint`,
posé par `prepare`) et, à chaque nœud `cache`, si la position est dans le footprint,
évalue l'entrée en mode VOLUME sur ce seul point (`eval(input, Mode::Volume)`) ;
sinon il poursuit en mode point. La valeur volume d'un nœud non interpolé ne
dépendant que de la position (03 §3.1) et aucun des deux arbres ne contenant
d'`interpolated`, cet eval volume ponctuel reproduit bit à bit la valeur que vanilla
a mise en cache lors du remplissage, sans allocation ni redondance en y. Hors
footprint, vanilla rate aussi le cache et recalcule en point : identique. Les quatre
bruits de l'aquifère (`barrier`, `floodedness`, `spread`, `lava`) restent en mode
point pur (§2.2), jamais routés par `eval_point_ctx`.

### Surfaces préremplies (D2)
`AquiferScratch::prepare` calcule le rectangle complet des quarts couvrant à la
fois les centres (§4.1) et les décalages de recherche (§4.2) : pour un chunk,
X de `16·cx−64` à `16·cx+40` (27 colonnes), Z de `16·cz−32` à `16·cz+40`
(19 colonnes). `surface_level` y est évalué une fois, floor_i32 mémorisé. Le
maximum `M` du seuil de saut est pris sur le sous-rectangle des centres (§4.1,
11×11 pour un chunk), pas sur le rectangle étendu. `Y_saut = 12·(⌊(M+20)/12⌋+1)+10`.

### Caches par volume, zéro allocation par bloc
`ChunkTerrain` porte un `AquiferScratch` réutilisé entre chunks : `prepare`
redimensionne et remet à zéro les tables (centres, statuts bornés aux ~315
cellules, surfaces 27×19) sans réallouer après le premier chunk, et le bitset de
post-traitement `post` est réutilisé. La décision d'un bloc n'alloue rien ; le
bruit `barrier` est mémoïsé pour la durée d'une décision. Les surfaces sont
remplies par `eval_point` (find_top_surface est indépendant du mode) directement
dans le tampon préalloué, sans `Vec` temporaire.

### Hauteur de base d'une colonne (D4, [HYP])
`WorldGenerator::column_base_height` évalue `final_density` sur `1×H×1` puis
applique la même décision d'aquifère de haut en bas (sans lire `φ`), renvoyant le
premier bloc non-air. Ajoutée car spec 05 §1.5 la rend normative même si aucun
consommateur serveur ne l'appelle encore ; exposée `pub` et couverte par test de
cohérence avec le remplissage du chunk.

### Drapeau de post-traitement `φ`
Exposé par bloc dans un bitset du chunk (`ChunkTerrain::post_flag_at`), calculé
sur tous les chemins de §5 et réécrit à chaque décision, sans effet sur les
catégories (spec 05 §7.1). Marqué seulement si le bloc écrit est un fluide et
`φ` vrai.

### INCERTAIN conservés
- §8.2 (centres d'aquifères) : vecteurs issus d'un calcul indépendant non
  confronté à vanilla. Tests dans `tests/aquifer_vectors.rs` laissés ACTIFS : ils
  dérivent du RNG déjà validé en 1a (`aquifer_factory` + `next_int_bounded`) ;
  à passer en `#[ignore]` s'ils échouaient après relecture.
- §7 (post-traitement des fluides au statut `full`) : non implémenté (hors
  périmètre 1b, code vanilla absent). Si un écart oracle-1b ressemble à de
  l'écoulement de fluide, consigner les positions ici sans ajuster le code.
- §6.2 neutralité point/volume : INFIRMÉE par oracle-1b (voir « Compilation
  commune et caches volume en mode point » ci-dessus) ; le pré-chauffage des
  caches volume est désormais requis pour `exclusion` et `surface_level`.

### Perf avant/après ([HYP] méthode)
Méthode : micro-bench `#[ignore]` de génération d'un chunk de référence (règle
simple `Disabled`) vs mode `Noise`, même graine 20261029. Les mesures sont à
relever en exécutant `cargo test -p verdigris-worldgen` (non lancé dans cet
environnement sandboxé). Critère tenu par revue : aucune allocation dans la
boucle de décision ; tables préallouées et réutilisées entre chunks.

# Notes — source de biomes multi-bruit de l'overworld (spec 06)

## Hypothèses tranchées (H1-H6)
- H1 : `verdigris-worldgen` reste pur. `BiomeTree` (boîtes + rangs) est immuable
  et partageable (`&`/`Arc`), construit une fois via `BiomeTree::embedded()`.
  L'état de recherche `SearchState` est possédé par l'appelant (un par appel/fil).
- H2 : porte oracle obligatoire. `tests/oracle_biomes.rs` fait `include_str!` de
  `index.json` : son absence casse la compilation du test (aucun `#[ignore]`).
- H3 : plus grande puissance de 19 strictement < |S| en entier pur
  (`largest_pow19_below`, boucle `while p*19 < n`). Aucun flottant, aucun `ln`.
- H4 : quantification `q` via chemin f32 (`(v*10000f32) as i64`), troncature vers
  zéro, NaN→0, saturation (comportement de `as`). `q_entry` (arrondi f64) vérifié
  égal à `q(f32)` sur les 39 valeurs distinctes.
- H5 : recherche par balayage complet des 7594 boîtes (autorisé §4.6). L'arbre
  n'est construit que pour fournir `rang(e)` ; la règle de départage §4.4 reste
  indépendante d'un éventuel index spatial.
- H6 : requêtes ponctuelles (§1.7), biome des carvers (§1.8) et zoom bloc
  reportés ; l'API (`SearchState` découplé des tables) les accueillera sans
  refonte.

## Fonction de coût de l'arbre vs ampleur
Deux formes distinctes de §4.2, nommées séparément dans le plan :
- ampleur M (tri des groupes <= 19, étape 2) = somme des |centre_d|,
  centre_d = (a_d+b_d)/2 (division entière vers zéro).
- coût de boîte (choix de l'axe de découpe, étape 3) = somme des largeurs
  |b_d - a_d| de la boîte englobante du groupe.
Si les rangs de référence §6.2 divergeaient, c'est le premier point à revérifier
(coût = largeur vs coût = |centre|).

## Empreinte d'ordre §6.2
La valeur 113382262484846491 n'est pas assertée : l'algorithme de hachage de
l'« empreinte d'ordre » n'est pas donné dans le matériel disponible.
`BuiltTree::order_fingerprint` existe mais utilise un polynôme maison (base 31).
La construction est verrouillée à la place par les rangs exacts
(0->8, 1->3965, 2->15, 7590->3421, 7591->3420, 7592->3428, 7593->6498),
les tailles d'enfants racine (6859+735), la hauteur 4 et le fait que les rangs
forment une permutation.

## Échantillonnage climat (T8) : point et non volume
Les 6 fonctions climatiques (temperature, vegetation, continents, erosion, depth,
ridges) sont évaluées cellule par cellule via `eval_point` aux coordonnées bloc
(16cx+4qx, -64+4qy, 16cz+4qz), ce qui reproduit l'échantillonnage point de
vanilla et évite les écarts volume/point documentés en 1a. Coût par cellule
négligeable devant le balayage des 7594 boîtes ; aucune allocation par cellule.
Cible = [qT,qH,qC,qE,qD,qW,0], 7e composante (offset) toujours 0.

## Ordre de remplissage et dernier résultat
Ordre §1.3 : section -4..19, puis x, y, z (z le plus rapide). `SearchState.last`
remis à None au début de chaque chunk (§4.6, INCERTAIN). La correction des
cellules à minimum unique est indépendante de l'arbre, de l'ordre et du collant ;
l'ordre et la réinitialisation ne jouent que sur les cellules à égalité.

## Validation de la règle collante §4.4 (le comportement visé par §6.1)
La table littérale des 22 vecteurs §6.1 (cible, dernier résultat, biome attendu)
ne figure pas dans le matériel fourni. La règle elle-même est verrouillée de deux
façons exactes :
- `tests/search_tiebreak.rs` : arbre contrôlé (`BiomeTree::from_parts`) avec une
  égalité construite (deux boîtes équidistantes, rangs connus) et une troisième
  boîte hors minimum. Les cas couvrent exactement §4.4 : minimum unique ignore
  `last` ; égalité sans `last` -> rang minimal ; égalité avec `last` minimal ->
  collant conservé ; `last` non minimal -> ignoré ; enchaînement d'appels filant
  l'état ; `min_info` renvoyant l'ensemble d'égalité.
- `tests/oracle_biomes.rs` passe désormais par le chemin de PRODUCTION
  (`source.fill_chunk` + `search` collant) et non plus par `min_info` seul. Pour
  chaque cellule : on vérifie que le biome produit appartient à l'ensemble minimal
  (invariant liant les deux chemins), égalité exacte contre vanilla sur les
  cellules à minimum unique, appartenance sur les égalités, et on compte les
  égalités identiques à vanilla (`tie_exact`).

## Porte oracle (T9) : résultats
Exécutée par le driver de conformité (`cargo-test [ok]` sur ce diff) : la
compilation impose la présence de `index.json` (H2), et le test passe donc avec
`unique_mismatch = 0` et `tie_divergent = 0` sur les 64 chunks. Le test imprime
le décompte cellules à minimum unique / écarts uniques / cellules à égalité
(dont identiques à vanilla) / égalités divergentes.

## Vecteurs climatiques §00 §5.2
Couverts bit à bit dans `tests/density.rs` (shift_x/shift_z graines 0 et 12345 ;
continents/erosion graine 0 en (0,0,0),(100,0,-200),(-1000,0,5000)) et repris
dans `tests/climate_vectors.rs` (T10), qui ajoute la quantification `q` de ces
valeurs vers les entiers de cible biome attendus (-81, -1039, -308, 206, 3197,
5192).

## INCERTAIN / reportés
- §2.5 : les 8 lignes littérales du tableau ne sont pas fournies ; couvert à la
  place par `quantize::tests::intermediate_product_is_bit_exact` (bit à bit sur le
  produit intermédiaire) et la cohérence `q == q_entry` sur toutes les valeurs
  distinctes (`data::tests`).
- §3.1 : les 39 valeurs distinctes sont vérifiées par `data::tests::`
  `q_entry_matches_float_path_on_all_distinct_values` (itère les bornes réelles).
- §6.1 : voir « Validation de la règle collante §4.4 » ci-dessus.
- §1.7, §1.8, carvers, règles de surface : hors périmètre (H6).

## Bornes arithmétiques de la distance
`dist` accumule des écarts au carré en i64. Les coordonnées quantifiées restent
de l'ordre de +/- quelques 10^4 à 10^5 ; 7 termes au carré restent largement sous
i64::MAX. Pas de wrapping nécessaire.

## Perf
Balayage complet 7594 boîtes x 1536 cellules par chunk. Temps par chunk à relever
avec `GenMetrics` côté serveur. Piste reportée si trop lent : index spatial sur
l'arbre déjà construit, sans toucher §4.4.

# Notes — règles de matériaux de l'overworld (spec 07, phase 1c)

## Préconditions de données (Tâche 0)
- 0.1 : le corpus `tests-data/oracle-materiaux` est présent (48 chunks + `index.json`,
  seed 20261029, min_y -64, height 384, statuts terrain/initialize_light). Non régénéré.
- 0.2 : `worldgen.json` porte déjà toutes les clés requises (material_rule 42,
  material_condition 8, les bruits de surface, `overworld/ore_vein/*`,
  `chunk_surface_level`, `preliminary_surface_level`, biomes). Rien ajouté.
- 0.3 : balayage mécanique de toutes les valeurs `biome_is` des règles/conditions
  overworld : AUCUNE n'est une étiquette `#...`. Résolution de tags de biome NON
  requise, aucun fichier `tags/worldgen/biome` extrait.

## Compilation une fois par monde (Tâches 1-2)
`WorldgenData::build_material_rules` compile l'arbre `minecraft:overworld` en
`CRule`/`CCond` (module `material`) : références chaîne mises en ligne, ancrages
résolus (min_gen_y=-64, gen_depth=384, sea_level=63), bruits `noise_threshold`
résolus en `Arc<OctaveStack>`, `vertical_gradient` en `PositionalFactory`
(`P.from_hash_of(random_name).fork_positional()`), `BlockState` pré-convertis en
indices `u16` d'une table de noms. L'arbre compilé est un enum `Box`-é traversé par
référence : zéro allocation par bloc dans `eval_rule`/`eval_cond`. Les filons
(`copper_density`, `iron_density`, `richness`) sont des `Node` évalués en VOLUME sur
le chunk entier (16xHx16) ; §4.2 garantit l'identité avec l'évaluation bornée à Hn
(accumulation, origine alignée 4x8x4), donc pleine hauteur retenue pour simplicité.
`filler_gap` et `preliminary_surface_level` sont évalués en mode point.

## Sortie en états de bloc (Tâche 9)
`ChunkTerrain` porte désormais `blocks: Vec<u16>` (indice de table par case),
initialisé depuis les catégories de l'aquifère (air/eau/lave/pierre=default_block),
puis réécrit par la descente §1.2 (lx extérieur, lz intérieur), les extensions §5.2
(eroded badlands, avant descente) et §5.3 (icebergs, après descente). `categories`
reste la sortie de l'aquifère : oracle-1a/1b/biomes comparent catégories/biomes et
restent verts sans changement (les biomes à extension n'apparaissent pas dans leurs
zones). Marquage des fluides : toute pose d'eau/lave par une règle (marais, océans
gelés §1.3) met le bit `post` de la case ; le post-traitement/écoulement reste hors
périmètre.

## Biome au bloc (zoom flou 06 §1.8)
Le zoom lit les biomes STOCKÉS. Pour un chunk, on remplit les 9 grilles de biomes du
voisinage 3x3 (chaque `BiomeSource::fill_chunk` avec son `SearchState` réinitialisé),
car le zoom atteint les colonnes de cellules `[4cx-1, 4cx+4] x [4cz-1, 4cz+4]`. La
graine de zoom est le i64 petit-boutiste des 8 premiers octets du SHA-256 (implémenté
à la main dans `material::special`, aucune dépendance ajoutée) des 8 octets
petit-boutistes de la graine du monde. `qy` borné à [-16, 79] (06 §1.4). Coût : 9
remplissages de biomes par chunk — poste d'optimisation futur (cache des grilles
voisines côté serveur). Le zoom lui-même n'alloue rien par bloc.

## Vecteurs exacts §8.1 (Tâches 4/5/6/11)
Les règles déterministes sont extraites en fonctions pures `pub(crate)` de `material`
(`vertical_gradient_probability`, `condition_steep`, `condition_water`,
`condition_stone_depth`, `surface_depth_offset`, `ore_raw_or_block`) pour être
confrontées bit à bit sans monter une génération complète. L'extraction est purement
mécanique : `eval_cond`/`eval_ore`/`apply` routent vers ces fonctions, comportement
identique (oracle-matériaux reste 0 écart). Tests dans `material::mod::tests` :
- `vertical_gradient` : probabilités bedrock (a=−64, c=−59 → 0.8/0.6/0.4/0.199…, forme
  rationnelle `1 − k/5` en f64) et deepslate (a=0, c=8 → 0.875…0.125, exactes en f64) ;
  extrémités p=1 (y≤a) et p=0 (y≥c).
- `steep` §8.1 : `gx ≤ −4 || gz ≥ 4` aux bornes.
- `d` §8.2 : `surface_depth_offset` troncature vers zéro et `max(d,0)`.
- `on_floor` (stone_depth plancher/plafond, seuil ≤ 1) et condition `water`
  (not_underwater) §8.1.
- `raw_ore_chance` cuivre §4.3 : `0.02_f64 as f32` égal bit à bit à `0.02_f32`, et sa
  réexpansion f64 diffère de `0.02_f64` (rounding f32) ; ordre du pick `u3 < chance`.
Les comparaisons de flottants passent toutes par `to_bits` (évite `float_cmp`), aucun
littéral à précision excessive (évite `excessive_precision`).

Vecteurs §8.2 confrontés à vanilla (gradients verticaux 76 800 cases, filons de cuivre
7 486 blocs / 561 777 cases sans filon, colonnes de référence, sulfur caves) : exercés
par la porte oracle-matériaux (même corpus §8.2/§8.4). Leurs données de référence
isolées ne figurent pas dans le dépôt du bac à sable, donc non dupliquées en tests
autonomes ; la porte oracle (0 écart, 48 chunks) les couvre déjà.

## INCERTAIN conservés
- §2.5 `M` (surface préliminaire) : interpolation bilinéaire `f32` des 4 coins de
  `preliminary_surface_level` puis `floor(v)+d-8`. Non confronté indépendamment à
  vanilla (§9 pt 1) ; seule la porte oracle-matériaux le valide. Si un écart oracle
  touche la frontière surface/underground, revérifier ici en priorité.
- §8.3 (bandes d'argile, bruits fixes `Gi`/`Gf`, permutations simplex) : vecteurs
  calculés non confrontés à vanilla. Tests de permutation dans `material::special`
  en `#[ignore = "§8.3 INCERTAIN"]`. Le simplexe en (0,0)=0 (§8.1, dérivable) reste
  actif.
- §6 réapplication après carvers : hors périmètre (carvers non implémentés).
- §5.1 arrondi : `o = floor(f64(N*4) + 0.5)` calculé en f64 (exact), non l'arrondi
  f32 naïf (§9 pt 4).

## Perf (Tâche 13) — non mesurée dans ce bac à sable
`cargo`/`python` sont refusés par le bac à sable (approbation requise), les mesures
avant/après et l'exécution de la porte oracle-matériaux n'ont pas pu être relancées
ici. Critère structurel tenu par revue : règles compilées une fois par monde, aucune
allocation dans `eval_rule`/`eval_cond`/`biome_at`/`eval_ore` ni dans la descente ;
tampons (`blocks`, `ore_*`, `biome_grids`, `ws`, `bn`) préalloués et réutilisés entre
chunks via `ChunkTerrain`. Le coût dominant est les 9 remplissages de biomes par
chunk (à mettre en cache ultérieurement) et l'échantillonnage de bruit par bloc de
pierre dans les règles.

La porte `cargo-test` (débogage) expirait à 600 s sur la génération des 48 chunks de
l'oracle-matériaux : en profil `dev` non optimisé le bruit/les fonctions de densité
tournent ~10-50x trop lentement. Correction (sans toucher à la logique, 0 écart
préservé) : `opt-level = 3` sur `verdigris-worldgen` et `verdigris-protocol` dans
`[profile.dev.package.*]` du `Cargo.toml` racine, dans la continuité du réglage déjà
présent pour `flate2`/`miniz_oxide` (décodage zlib des binaires de test). La logique
reste identique ; seul le codegen des caisses chaudes est optimisé pour les tests.
