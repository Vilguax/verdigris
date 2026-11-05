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

# Notes — carvers de l'overworld (spec 08, module `carver`)

## Spec 08 disponible (session 2026-10-31) — MISE À JOUR
La spec salle blanche 08 est désormais accessible et fait foi. Les rejets précédents venaient d'un algorithme
reconstruit de la prose (géométrie divergente partout). La géométrie du masque a été
RÉIMPLÉMENTÉE à partir des §1 à §5 et verrouillée par les vecteurs §8.1 (table de sinus)
et §8.2 (réensemencement, départs grotte/canyon, comptes de masque 111/1403/1122/3636/3237)
dans `tests/carver_vectors.rs` et `carver/sincos.rs`. Vecteurs et porte oracle exécutés
par le pilote externe : tous verts. Corrections clés vs la reconstruction précédente :
- grotte `L = 112 − ni(28)` (range=4), plus 240 ;
- table de sinus `T[k] = f32(sin(f64(k)/10430.378350470453))`, lookup
  `tronc64(f64(a)×10430.378350470453) AND 65535`, empreinte SHA-256 §8.1 assertée ;
- ellipsoïde §3.2 rejet rapide `δ = 16 + r_h×2`, saut de colonne `xd²+zd² ≥ 1` ;
  atteinte §3.3 `R = f64(f32(f32(t+2)+16))` (f32 puis élargi) ;
- prédicat grotte §4.6 `yd > p && (xd²+yd²+zd²) < 1` (le `−0.7` parasite retiré) ;
- canyon §5.1/§5.3 `L = tronc(112×distance_factor)`, `r_v = (κ×base×y_scale)×U(0.75,1)`,
  `r_h = base×horizontal_radius_factor`, table `FL` de 384 (gen_depth) entrées, prédicat
  §5.4 diviseur 6.0 indice `FL[y+63]` ;
- hauteurs de départ résolues avec `min_gen_y=−64`/top=319 (07 §2.6), pas les bornes du
  masque ; cap `θ = nf × TAU`.
Les points marqués INCERTAIN ci-dessous (spec §9) ne sont pas confrontables en session ;
clause T3 : ne PAS ajuster le code pour forcer un vecteur ou l'oracle.

## Porte oracle-carvers (clause T3)
`tests/oracle_carvers.rs::carvers_match_oracle` exige l'égalité exacte des identifiants
sur 48 chunks, avec diagnostics (écarts par paire attendu/obtenu, premières positions).
Active et verte : 0 écart sur 48 chunks. Contrôle négatif : la même porte avec carvers
désactivés relève 46543 écarts, la porte mesure donc bien le creusement.
Clause T3 : ne tweak AUCUNE constante du carver pour chasser l'oracle.

## H1 — données en lecture seule
`worldgen.json` porte déjà `carver.{cave,cave_extra_underground,canyon,nether_cave}`
et le champ `carvers` par biome (overworld : liste des 3). `network.json` porte le tag
`minecraft:uncarvable` = ids [36] = `minecraft:bedrock` (résolu via `blocks.json`).
Aucune écriture, aucun `build.rs`. `CarverData::from_worldgen` lit le `Value`
embarqué ; `network.json`/`blocks.json` sont `include_str!` dans `carver/mod.rs`.

## H2 — résolveur de biomes threadé
`carve_chunk_mask_into(data, biomes, state, seed, cx, cz, mask)` prend le
`BiomeSource` déjà construit (réutilisé depuis `MaterialRules.biome_source`). Pour
chaque chunk source (rayon 8), la liste `carvers` est lue en POINT via
`BiomeSource::biome_at_point(state, 16·sx, 0, 16·sz)` (06 §1.7 : quart `fromBlock`).
Le court-circuit vanilla (toutes les listes overworld identiques) est permis mais non
pris : le chemin générique par biome tourne toujours (correct datapack).

## H3 — réutilisation de l'aquifère du remplissage
Le creusement n'instancie aucun aquifère. `carver::apply::apply_carvers` rappelle
`AquiferScratch::decide(funcs, x, y, z, 0.0)` sur l'instance `out.aquifer` déjà
`prepare`-ée par `fill_chunk` (mode Noise). `Filled::Nothing` => case non creusée ;
`Air/Water/Lava` => bloc posé, bit `post` mis si `φ` et fluide. C'est l'équivalent
de `aquifer.computeSubstance(ctx, 0.0)` de vanilla. Le creusement est intégré en fin
de `fill_chunk` précisément pour réutiliser ce contexte sans ré-exposer l'aquifère.

## H4 — option générateur
`WorldGenerator::with_options(seed, mode, carvers)`. `new()`/`with_mode()` gardent
`carvers=false` : oracle-1a/1b/biomes/matériaux et les tests serveur passent par elles
et restent inchangés (T2). Le serveur (`GeneratorSetup::build` Noise) construit avec
`carvers=true` (défaut-on, E1). Le creusement n'est actif qu'en mode `Noise` (aquifère
préparé requis).

## Masque (D1)
`carver/mask.rs` : bitset 16×16×376, Y_min=−63, Y_max=312, index
`(y−Y_min)+(lz+16·lx)·376` (ordre colonne lx extérieur, lz intérieur). Ces bornes
sont figées par le plan (la couche bedrock −64 est hors creusement). La table FL du
canyon (§5.2) est indexée `y−Y_min` = `y+63`, sans `ni` pour j=0, resample si
`ni(width_smoothness)==0`.

## RNG (§2.1) et table de sinus (§3.1)
`seed_large_feature(S+i, sx, sz)` = LCG(S+i) -> a=nextLong, b=nextLong,
g = (sx·a) ^ (sz·b) ^ (S+i) (i64 wrapping) -> LCG(g). `is_start` = `nextFloat ≤ probability`.
Les tunnels se réensemencent par `nextLong` (nouveau LCG). Table de sinus (§3.1 exacte) :
`T[k] = f32(sin(f64(k)/10430.378350470453))` ;
`sin_t(a)=T[(f64(a)×10430.378350470453) as i64 & 65535]`,
`cos_t(a)=T[(f64(a)×10430.378350470453 + 16384.0) as i64 & 65535]`. Les tests `carver/sincos.rs`
assertent bit à bit les `T[k]` de §8.1 (0/1/100/12345/16384/32768/49152/65535), les `sin_t`/`cos_t`
de §8.1, et l'empreinte SHA-256 `87a67a39…dac2` via un SHA-256 écrit dans le module de test
(aucune dépendance ajoutée). Réserve §9#3 : l'empreinte est celle de la JVM 25 ; si le libm
f64 de la plate-forme diffère d'un ULP, passer le test d'empreinte en `#[ignore]` (les `T[k]`
individuels restent les garde-fous durs).

## Algorithme — résolu par la spec §4/§5 (plus de guess)
- Longueur grotte `L = 112 − ni(28)` (§4.1), canyon `L = tronc(112×distance_factor)` (§5.1).
- `get_thickness` : `thickness.sample` (2 nf) puis si `weird_thickness_bias && ni(10)==0`,
  `t = t × f32(f32(f32(n1×n2)×3)+1)` avec 2 nouveaux nf (§4.1).
- `start_vertical_radius_multiplier` = e_0, échelle verticale du 1er tunnel (vanilla 1.0,
  pas de tirage, §4.1 pt 6 / §4.5) ; branches e=1.0.
- Bifurcation §4.4 : gauche `g1=nextLong`, `t1=f32(f32(nf×0.5)+0.5)` puis droite `g2`,`t2` ;
  caps `θ∓π/2`, pente `φ/3`, pas initial s_b, e=1.0.
- Canyon §5.3 : `r_h = base×horizontal_radius_factor` (1 nf), `r_v = (κ×base×y_scale)×U(0.75,1)`
  (1 nf) ; distance_factor NE sert QU'À `L`. κ = D + C·μ (vanilla D=1,C=0 → κ=1). Prédicat §5.4
  `(g²+h²)·FL[y+63] + d2²/6 < 1`. Table FL de 384 entrées (pas de ni pour j=0).
- Constantes de marche confirmées §4.3/§5.3 (grotte pitch×0.92/0.7, ±0.1, dθ×0.9, dφ×0.75, 2/4 ;
  canyon ×0.7, ±0.05, ×0.8/×0.5) et les littéraux f32 de §8.1.

## §6 réapplication des matériaux de dessus — best-effort (07 §6 contexte réduit indisponible)
`apply_carvers` suit l'ordre §1.5 : colonnes lx ext / lz int ; séries (suites maximales
marquées) de la plus basse à la plus haute ; dans une série du haut vers le bas :
- drapeau `reached_surface` (H) remis à faux AU DÉBUT DE CHAQUE SÉRIE, mis à vrai quand la
  case creusée portait `grass_block` ou `mycelium` (avant l'aquifère) ; le bloc déclencheur
  est mémorisé.
- réapplication du matériau de dessus : quand H et que la case directement sous une
  case creusée en air est `dirt`, cette dirt devient le bloc de surface mémorisé
  (grass/mycelium) et repasse solide. Approximation fidèle au cas couvert par H (en
  vanilla `reachedSurface` n'est posé que par grass/mycelium, donc `topMaterial` y
  rend justement ce bloc) ; la règle biome-complète 07 §6 (contexte réduit) n'est pas
  disponible, cette version ne couvre donc que la relocalisation du bloc de surface,
  pas les surfaces spéciales (sable, badlands, etc.) qui ne posent pas H.
- heightmap de surface : `ChunkTerrain.ws` (WORLD_SURFACE de travail) est recalculé
  par colonne après creusement (premier non-air par le haut). Aucun tableau WG séparé
  n'est matérialisé car aucun consommateur ne le lit encore ; `ws` porte l'info.
- bit `post` (correction revue) : UNIQUEMENT posé si `φ` et fluide (§1.6). La branche
  qui effaçait `post` sur une case air posée a été retirée (effacement non spécifié).

## §8 vecteurs — transcrits (actifs)
`tests/carver_vectors.rs` transcrit §8.2 en égalités bit à bit : table de réensemencement
(a, b, g, premier nf) pour graines 0 et 20261029 ; départ grotte (−10,−5) i=0 (count N=7,
x/y/z, m_h/m_v/p, θ/φ/t/L/graine) ; départ canyon (−5,−8) i=2 (x/y/z, θ/φ/t,
distance_factor, L, graine, FL[0..3]) ; comptes de masque 111 (grotte), 1403 (canyon),
1122/3636/3237 (chunks complets). §8.1 est dans `carver/sincos.rs`. Tous ACTIFS ; un vecteur
qui résisterait après relecture => `#[ignore = "raison"]` (H5/T3), jamais d'ajustement du code.

## Perf (E2)
Mesure indicative (release, graine 20261029, 48 chunks de la porte oracle, temps total du
test chargement compris) : 21,6 s avec carvers, 21,2 s sans, soit environ 8 ms par chunk
pour le masque et son application. Critère structurel : masque fonction
des seules sources, table de sinus et bitset préalloués (OnceLock / champ réutilisé de
`ChunkTerrain`), aucune allocation par bloc dans `ellipsoid`/`apply_carvers` (closures
monomorphisées, pas de `Vec` par case ; le seul `Vec` par carve canyon est la table FL
de 376 f32, une fois par tunnel). Méthodo à relever : même zone, graine 20261029, N
chunks, moyenne/médiane, avant/après ; validation d'exactitude par l'oracle, pas par le
temps.

# Notes — décoration (features et placed features, module `feature`)

## Générateur R (spec 09 §3)
`FeatureRng` enveloppe le Xoroshiro existant mais expose l'interface générique
`java.util.Random` : `next(bits)` = `xoroshiro.nextLong() >>> (64 − bits)` (bits
hauts), puis `next_int`, `next_long`, `next_float`, `next_double`, `next_int_bounded`,
`next_gaussian` dérivent de `next(bits)` par l'algorithme générique, PAS par les
méthodes natives du Xoroshiro. Conséquences vérifiées bit à bit (§11.2, graine 0) :
`next_long() = ((long)next(32) << 32) + next(32)` (≠ nextLong brut), `next_float() =
next(24)·2⁻²⁴`, `next_double() = ((next(26) << 27) + next(27))·2⁻⁵³`. La séquence
testée réensemence à chaque primitive (`next_long` puis `next_float` puis `next_double`
enchaînés) donne mantisses entières 4214372 (float) et 1054967193155086 (double) ;
testées en entier pour éviter `excessive_precision`. `set_seed` NE vide PAS le tampon
gaussien (§3.5). Graine de population `set_decoration_seed` : `D = (minX·(l|1) +
minZ·(m|1)) ^ graine` ; en (0,0) `D = graine` (§11.1). Graine de feature `D + i +
10000·k` (§3.3), `i` = position dans la liste triée de l'étape.

## Liste globale (§2.2/§2.5)
`order::build_order` reproduit le tri topologique vanilla (FeatureData `(étape,
index)`, index d'apparition global, arêtes consécutives dans la liste aplatie d'un
biome, DFS post-ordre sur clés triées, inversion, découpage par étape, 11 étapes de
décoration, cycle → `Err`). L'ordre des biomes qui pilote le tri (et donc les index
globaux `i`) est l'ordre des biomes possibles de l'overworld, pas l'ordre alphabétique
du registre : `overworld_biome_order` prend les biomes distincts dans leur ordre de
PREMIÈRE apparition dans `biome_entries()` (liste de paramètres climatiques overworld),
ce qui reproduit `possibleBiomes()` de la source de biomes utilisée par le trieur
vanilla. 56 biomes distincts, 171 entrées de placed features, 11 étapes : asserté
exactement (`embedded_global_order_matches_2_5_counts`). L'erreur précédente pilotait
le tri par les clés triées (BTreeMap alphabétique) de `biome_features`.

## Types 26.3 et règle §r (CORR 1/4)
Cinq espaces de noms disjoints avec liste écrite dans le code : placement_modifier
(18), block_state_provider, block_predicate (`type`), rule_test (`predicate_type`),
feature (compositions implémentées, le reste en feuille brute). La règle §r rejette
tout type absent de la liste de SON sous-système, à l'endroit syntaxique où il est
parsé : les modificateurs de placement (`placed_feature.placement`) et le prédicat de
`block_predicate_filter` sont validés au chargement (erreur explicite sur type
inconnu) ; `validate_rule_test` couvre les tests de règle. La liste feature n'est pas
exhaustivement transcrite : toute feature non-composition est chargée en feuille brute
(H5), donc le chargement ne rejette jamais un type de feature réel (critère 1) ; §o
est un test de complétude des listes placement/provider/predicate contre les données.

## Features connues non implémentées (H5)
Les features qui posent des blocs (ore, disk, tree, lake…) sont chargées (config JSON
brute conservée) et exécutées comme échec sans pose : `place_feature` d'une feuille
enregistre la position atteinte dans le contexte (visibilité des tests) et renvoie
`false`. Les compositions (`no_op`, `sequence`, `overlay`, `random_selector`,
`simple_random_selector`, `weighted_random_selector`, `random_boolean_selector`) sont
implémentées : elles tirent leur propre aléa (sélecteurs) puis descendent dans leurs
enfants, sans rien poser. `sequence` s'arrête au premier enfant qui renvoie `false`
(donc dès la première feuille, les enfants suivants ne tirent pas), `overlay` exécute
tous ses enfants et réussit si l'un réussit (§9.1, testé par
`sequence_stops_at_first_failure_overlay_runs_all`). Comme chaque placed feature de
1er niveau réensemence sa graine, l'aléa interne non tiré des feuilles n'affecte pas
les autres features ; les vecteurs §11.2 portent sur les tirages des modificateurs et
les graines, exacts. `set_feature_seed` = `set_seed(D + i + 10000·k)` vérifié
(`feature_seed_matches_formula_3_3`, 4× `next_int(16)`).

## Moteur de placement (§4.1/§9.1) et ordre des tirages
Parcours en profondeur (équivalent au flatMap paresseux de vanilla) : chaque position
issue du modificateur k traverse tous les modificateurs suivants avant la position
suivante. `count` tire une fois puis réplique, `in_square` tire 2× `next_int(16)`,
`height_range` tire via le fournisseur de hauteur (ancrages résolus min_y=−64,
height=384), `offset` tire x/y/z, `rarity_filter`/`random_chance` tirent un `next_float`.
Les filtres sans tirage (`biome`, `heightmap`, `block_predicate_filter`) laissent passer
ou coupent. `heightmap`/`block_predicate_filter` lisent la région ; toute erreur de
lecture (distance chunk > 8, etc.) remonte en `Err` jusqu'à `decorate_chunk`.

## Contexte réutilisable (§g)
Le parcours est par visiteur récursif : aucune allocation par position (pas de `Vec`
intermédiaire par modificateur). `DecorationContext` porte un unique tampon de
positions atteintes, vidé (jamais rétréci) au début de chaque `decorate_chunk` ; sa
capacité est inchangée au 2ᵉ passage (test). Les modificateurs et compositions sont
précompilés une fois (`Decoration::build`), pas reparsés par position.

## INCERTAIN / reportés
- §2.5 : comptes exacts (56 biomes, 171 entrées, 11 étapes) assertés et ordre des
  biomes conforme (ordre des paramètres overworld = `possibleBiomes` vanilla) ; la
  table littérale index-par-index n'est pas dans le matériel, mais la liste est dérivée
  de façon déterministe de la même source de biomes et du même trieur que vanilla.
- `next_int_bounded(n)` renvoie 0 si `n <= 0` au lieu de paniquer : une plage dégénérée
  (ex. uniforme `min > max`) ne fait plus paniquer la génération ; pour `n > 0` le
  comportement (et les vecteurs §11.2) est inchangé.
- `very_biased_to_bottom` : formule `i + nextInt(nextInt(span) + inner + 1)`,
  non confrontée ; marquée à revérifier.
- `trapezoid` (hauteur/entier) : pas de court-circuit `plateau ≥ étendue` ; min=max=0
  consomme 2 tirages et vaut 0 (§11.1, tranché par le plan, CORR 3).
- Filtre `biome` du moteur : désormais actif, applique le zoom flou par position (voir
  « Agrégation des biomes et zoom flou » ci-dessous) ; l'activation par étape utilise
  l'ensemble agrégé des 9 chunks que `decorate_chunk` calcule lui-même.
- Empaquetage des faces de `physics.json` : bit `(index_face · 6 + index_direction)` ;
  convention retenue faute de vecteur de contrôle, à confronter si un prédicat de face
  diverge.

## Complément phase 1b (socle features) — modificateurs, prédicats, types

### Modificateurs §5 (plus de laisser-passer)
Tous les modificateurs des données overworld 26.3 sont exécutés en propre, dans l'ordre
de parcours en profondeur (le modificateur k produit une position, qui traverse tous les
modificateurs suivants avant la suivante) :
- `count`, `rarity_filter`, `random_chance`, `in_square`, `height_range`, `heightmap`,
  `offset`, `biome`, `block_predicate_filter` : inchangés (tirages déjà validés §11.2).
- `count_on_every_layer` (§5.2) : re-tirage du fournisseur `count` à CHAQUE test de
  boucle `i < count.sample(R)` (quirk Java), `nextInt(16)` x puis z par item, position
  verticale par `find_on_ground` (balayage descendant depuis `OCEAN_FLOOR_WG`, premier
  creux air/remplaçable au-dessus d'un bloc `legacy_solid`, (layer+1)-ème occurrence) ;
  arrêt quand une couche ne pose rien. Implémenté MÊME absent des placed features
  overworld (exigence §5). [INCERTAIN] : `find_on_ground` reconstruit de la prose, non
  confronté à vanilla (pas d'oracle features ici).
- `noise_threshold_count` (§5.3) : `G = Simplex(graine 2345).value(x/200, z/200)` (bruit
  d'info biome, réutilisé de `material::special`), `count = G < noise_level ? below : above`,
  `count` copies à la même position, aucun tirage `R`.
- `noise_based_count` (§5.4) : `G = Simplex(2345).value(x/noise_factor, z/noise_factor)`,
  `count = ceil((G + noise_offset) · ratio)`, borné ≥ 0, aucun tirage `R`. [INCERTAIN]
  `G` est en `f32` (décision `material`), élargi en `f64` ; non confronté à vanilla.
- `surface_relative_threshold_filter` (§5.6) : `y ∈ [surface+min, surface+max]` en `i64`,
  `min` défaut `i32::MIN`, `max` défaut `i32::MAX`, aucun tirage.
- `surface_water_depth_filter` (§5.7) : `WORLD_SURFACE_WG − OCEAN_FLOOR_WG ≤ max_water_depth`.
- `environment_scan` (§5.5) : balayage dans `direction_of_search` jusqu'à `max_steps`,
  émet la première position où `target_condition` est vraie ; s'arrête hors monde ou dès
  que `allowed_search_condition` devient fausse. Aucun tirage.
- `randomly_selected` (§5.x) : `pick = nextInt(len(placements))` puis applique le
  modificateur choisi, qui enchaîne sur la queue des modificateurs. [INCERTAIN] le tirage
  (`nextInt(n)` uniforme) et l'absence de pondération sont reconstruits de la prose §3.3.
- `fixed_placement` (§5.18) : émet chaque position absolue configurée (filtrage au chunk
  délégué au rayon d'écriture). Implémenté MÊME absent des placed features overworld
  (exigence §5). [INCERTAIN] filtrage chunk non confronté.
- `cuboid` (§5.17, présent) : tire `xz_size` puis `y_size`, parcourt le pavé centré,
  émet arêtes/intérieur selon `include_edges`/`include_interior`. [INCERTAIN] ordre des
  tirages et géométrie reconstruits de la prose, non confrontés.
- `carving_mask` : ABSENT des données overworld 26.3. Non exécuté ; toute occurrence
  future provoque une erreur de chargement propre (`UnknownType`) plutôt qu'un bras par
  défaut (plan T3.2).
Aucune allocation par position : parcours par `split_first` sur une tranche de
modificateurs (pas de `Vec` intermédiaire), tampon `placed` unique réutilisé entre chunks.

### Prédicats §7 (plus de bras `=> true`)
`BlockPredicate` est compilé en ensembles d'états précalculés (`matching_blocks` et
`matching_block_tag` → plages `[first_state, first_state+state_count)` via `blocks.json`,
tags résolus une fois à la construction). L'évaluation lit l'état `u16` renvoyé par la
région (identifiant d'état global) et consulte `physics.json` : `solid` →
`legacy_solid`, `replaceable`, `has_sturdy_face(dir)` → `sturdy_full`, `matching_fluids`
→ nom du fluide via le registre `network.json`, `inside_world_bounds` → `y ∈ [−64, 320)`.
`offset` par prédicat (triplet, défaut `[0,0,0]`) appliqué à la position lue.
`all_of`/`any_of` court-circuitent dans l'ordre ; `not`/`true` exacts ; `unobstructed`
et `volume_match` → toujours vrais pendant la génération (décision explicite, documentée,
pas un laisser-passer silencieux). Récursion bornée à 64 (erreur `DepthLimit` au-delà).
[INCERTAIN] :
- `would_survive` : approximation « bloc dessous `legacy_solid` » ; la table §8.5 (dont
  `small_dripleaf`) n'est pas reconstruite faute de spec, non confronté à vanilla.
- `solid` = `legacy_solid` (§8.3 « solide historique ») : convention, à confronter si un
  prédicat `solid` diverge.
- `volume_match` traité comme `unobstructed` (vrai en génération) : 1 occurrence, non
  confronté.

### Types de features (§9, H1)
Les 55 types de features atteignables dans `features.json` (nommés + en ligne) sont
transcrits : 7 compositions implémentées (`no_op`/`sequence`/`overlay`/les 4 sélecteurs)
et 48 features posant des blocs (`FEATURE_LEAF_TYPES`). Un type de feature absent de ces
deux listes = erreur de chargement propre (`UnknownType`), plus de feuille brute
silencieuse. Les features posant des blocs sont RECONNUES au chargement (config conservée,
palette alimentée) mais compilées en `CompiledFeature::Unimplemented` : leur exécution ne
pose rien et incrémente `DecorationContext::unimplemented_hits` (observable, pas un faux
« connu »). `decorate_chunk` traverse malgré tout les 171 placed features pour exercer
l'ordre global et les tirages `R` ; il ne lève PAS `NotYetImplemented` (sinon la première
feuille avorterait la traversée, qui est le cœur de la phase 1b). La classification
explicite vit dans le registre (`COMPOSITION_TYPES`/`FEATURE_LEAF_TYPES`).

### Extraction récursive des états (§s, T4)
`FeatureRegistry::extract_states_rec` parcourt tout le JSON (bornée à 64) et interne tout
objet `{Name|id, Properties?}` dans la palette (fluid/barrier/ground_state/state/data et
fournisseurs imbriqués compris), erreurs ignorées (best-effort, non bloquant). Les états
en chaîne nue hors fournisseurs ne sont pas captés (reporté ; les feuilles ne posent pas).

### Agrégation des biomes et zoom flou (T5, critère figé)
`decorate_chunk(region, ctx, level_seed, cx, cz)` n'a PLUS de paramètre `candidate_biomes`.
Il agrège lui-même, via le trait `Region`, les biomes stockés des 9 chunks (`stored_biome`
sur chaque cellule 4×4 horizontale × 96 cellules verticales, `qy ∈ [-16, 80)`), dédoublonnés
dans un `BTreeSet` (spec 09 §2.3 : seulement les biomes présents dans les cellules). Le trait
`Region` gagne un `stored_biome(cell_x, cell_y, cell_z)` à impl par défaut
(`read_biome(cell<<2, …)`), donc aucun impl existant ne casse.

`is_active` ne garde un feature que si un biome candidat ∈ son ensemble autorisé ; biome
non résolu → faux (T7.1). Le modificateur `biome` (§5.10) applique le ZOOM FLOU (spec 06
§1.8) : `feature/zoom.rs::biome_at_block` reproduit bit à bit l'algorithme oracle-validé du
module `material` (`zoom_seed` du monde via SHA-256, `zoom_n`/`zoom_f`, sélection du coin de
score strictement minimal parmi 8), lit la cellule stockée élue via `Region::stored_biome`, et
ne laisse passer que si le biome au bloc ∈ l'ensemble autorisé de la feature. Une erreur de
région (lecture hors rayon) laisse passer sans avorter la traversée (documenté, non permissif
côté prédicats). Couvert par `tests/biome_block.rs` (activation plains, biome hors overworld
= 0, lecture effective des 8 chunks voisins) et `tests/global_order.rs` (56 biomes par ordre
de première apparition, 171 placed features, 11 étapes, ordre par étape cohérent). `decorate_chunk`
traverse quand même toutes les features pour l'ordre global + tirages `R` (les feuilles posant
des blocs incrémentent `unimplemented_hits`, jamais `NotYetImplemented` qui avorterait).

### Invariant perf (T0/T8)
Boucle de placement sans allocation par position : parcours `split_first` sur tranches, pile
implicite de récursion, tampon `placed` unique vidé (jamais rétréci) par chunk. `unsafe` étant
interdit par le règlement, l'allocateur compteur n'est pas utilisable ; repli documenté par
`tests/allocation.rs` (capacité du tampon stable sur 8 décorations identiques = pas de
réallocation par appel) + revue structurelle. L'agrégation des biomes alloue un `BTreeSet`
borné (~56 entrées) une fois par chunk, hors boucle chaude.

### Reports assumés
- `blocks.json` reste parsé par deux `OnceLock` (palette + tags), bénin ; mutualisation
  T7.2 non faite pour éviter un couplage inter-module.
- Zoom flou : réutilise l'algorithme du module `material` (porte oracle-matériaux verte) ;
  pas d'oracle de features dédié cette session, l'exactitude vanilla est héritée de `material`.
- Modificateurs/prédicats marqués [INCERTAIN] ci-dessus : aucun oracle de features dans
  cette session ; tests dédiés dérivés de la prose, à passer en `#[ignore]` s'ils
  divergent après relecture, jamais d'ajustement du code pour forcer un vecteur.

## Socle de décoration réelle — minerais et disques (spec 09 §9)

### H-région (D1, D2, spec 09 §1.2)
La région n'est pas une fenêtre 3×3 : elle raisonne en distance de Tchebychev
`d = max(|cx−Cx|, |cz−Cz|)`. `feature/region.rs::read_block` applique dans l'ordre :
borne verticale d'abord (`y < −64` ou `y ≥ 320` → `void_air`, D2 prime sans consulter
de chunk, y compris à `d ≥ 9`), puis `d > 8` → erreur, puis `d = 2..8` sans terrain → `air`
(et non `void_air`, D1), sinon le vrai bloc. Les voisins réels à `d = 2..8` restent lisibles
quand le terrain est prêt. `write_block` : `d ≥ 9` → erreur (pas un refus silencieux, D3) ;
`d = 2..8` → refus silencieux (`Ok(false)`, la feature continue) ; hors borne verticale →
`Ok(false)`. Le minerai teste explicitement le rayon d'écriture avant chaque bloc.

### H-biome-filtre (D5, spec 09 §5.10) — point corrigé
`collected_biomes` (9 chunks, §2.3) ne sert QU'À bâtir l'ensemble candidat `E_k` (gating
d'activation `is_active`). Le modificateur `minecraft:biome` est un filtre POSITIONNEL
distinct : pour chaque position `q`, `biome_allows` lit le biome zoomé (spec 06 §1.8) et
n'accepte que si ce biome référence la placed feature de PREMIER NIVEAU en cours. Le moteur
porte un « contexte de premier niveau » (`RunCtx.top_feature` = ensemble des biomes qui
référencent la feature de tête). Les features imbriquées (composition, sélecteurs) sont
exécutées via `RunCtx::nested()` qui met ce contexte à `None` : un `biome` imbriqué devient
une erreur fatale maîtrisée (aucune donnée 09 ne le déclenche). Une lecture de biome à
`d ≥ 2` remonte désormais une erreur (D5) au lieu de laisser passer : en pratique le filtre
tourne après `in_square`/`heightmap`, donc à `d ≤ 1`, et n'erre jamais. Correction par
rapport à l'ancien passthrough qui confondait sélection des candidats et filtre au bloc.

### Minerais `ore` (spec 09 §9.2)
`feature/features.rs::run_ore`. Ordre des tirages reproduit à la lettre : `θ = next_float()·π`
(`π` = le `f32` std), `y0`/`y1` via deux `next_int(3)`, puis par sphère `e = next_double()`.
`σ = size/8`, `ρ = ⌈(size/16·2+1)/2⌉`, `c = ⌈σ⌉` (tout en `f32`) ; `sin`/`cos` en `f64` de
`f64(θ)` pour les extrémités ; rayons via le sinus tabulé spec 08 §3.1
(`carver::sincos::sin_table().sin_t`). Élimination des sphères incluses, remplissage avec
masque `u²`, `u²+v²`, `u²+v²+w²` et `déjà_vus` représenté par un BITSET borné au pavé
englobant de la veine (`OreScratch.mask`, côté `t+1`, remis à zéro par veine), réutilisé
entre veines sans réallocation (tampons `xc/yc/zc/rad` redimensionnés, jamais rétrécis ;
`DecorationContext.ore`). Aucun court-circuit `d ≥ 2` avant la pose : chaque bloc du pavé
passe par le test de cible et la décision d'exposition (tirages consommés comme vanilla à
`d = 2..8`), puis `write_block` refuse silencieusement l'écriture hors rayon (D3). Décision
d'exposition : `δ ≤ 0` accepte ; `δ ≥ 1` accepte si aucun des 6 voisins n'est de l'air ;
sinon tirage `next_float()`, `≥ δ` accepte sinon test des voisins. Un voisin hors `[−64,319]`
compte comme air ; un voisin à `d = 2` est lu par la région.
Écriture DIRECTE en section (`Region::write_block`, sans mise à jour de carte). Vecteurs
§11.1 (`ore_metrics`) couverts : size 9 → (1.125, 2, 2, 8, 8), 17 → (2.125, 2, 3, 10, 8),
64 → (8, 5, 8, 26, 14).

### `scattered_ore` (spec 09 §9.3) et `disk` (spec 09 §9.4)
`scattered_ore` : `n = next_int(size+1)` ; par point `m = min(i,7)`, trois décalages
`arrondi(f32(next_float()−next_float())·m)` (arrondi Java = `(x+0.5).floor()`), réussit
toujours. L'ordre de tirage et les décalages §9.3 sont verrouillés par
`features_vectors.rs::scattered_ore_draw_order_and_offsets_11` (rejeu indépendant via les
primitives RNG validées § 11.2 : ensemble écrit == ensemble des positions dérivées de la
formule, sur plusieurs graines). Écart nominal assumé : la spec §9.3 décrit une pose ORDINAIRE
alors que l'implémentation écrit par `Region::write_block` direct comme `ore` — sans effet ici
(voir « Pose directe vs ordinaire » ci-dessous : aucune carte vivante n'est maintenue ni relue
dans ce socle). `disk` : `R_d = radius.tirer()`, colonne de `py+half_height`
jusqu'à `py−half_height` (décroissant), `target` (prédicat de bloc) évalué à l'état courant,
`state_provider` en « état facultatif » (dont `rule_based` : `disk_sand` = sandstone si le
bloc en dessous est air sinon sand). Vecteur §11.1 : disque rayon 2 = 13 colonnes.

### Tests de règle (§7.3) et fournisseurs d'état (§7.2)
`RuleTest` couvre `always_true`, `tag_match`, `block_match`, `blockstate_match`,
`random_block_match`, `random_blockstate_match`, `height_match`, `any_of`/`all_of`/`not`
avec arrêt au premier résultat décisif (les tirages des règles non évaluées n'ont pas lieu ;
`random_block_match` ne tire que si le bloc correspond). `StateProvider` couvre `simple`,
`weighted`, `random_block`, `rule_based` ; les types non requis par ore/disk restent
`Unsupported` et erreraient à l'exécution (jamais atteints par les features de ce socle).
Une feature `ore`/`disk`/`scattered_ore` dont la config ne parse pas retombe sur
`Unimplemented` plutôt que d'avorter le chargement (robustesse).

### Report gaussien (spec 09 §3.5)
`set_seed` ne vide pas le tampon de la méthode polaire : une gaussienne restante passe d'une
feature à la suivante dans le même chunk. `FeatureRng` conserve `next_gaussian`/`have_gaussian`
à travers `set_seed` (couvert par `rng.rs::set_seed_keeps_gaussian_buffer`). Aucune feature de
ce socle (ore/scattered_ore/disk) ne tire de gaussienne, mais l'invariant est structurellement
respecté pour les `clamped_normal` d'un socle ultérieur.

### Pose directe vs ordinaire (spec 09 §1.3) — simplification assumée
Les trois features écrivent par le même `Region::write_block` (chemin direct section). La
distinction §1.3 pose ordinaire (court-circuit section-d'air, mise à jour des 4 cartes
vivantes §1.4) vs écriture directe des minerais N'EST PAS matérialisée : aucune carte vivante
n'est maintenue. Justification de périmètre : dans le socle fer/minerais aucune feature
activée ne relit une carte vivante (le minerai et `disk_clay` lisent `OCEAN_FLOOR_WG` FIGÉE,
reconstruite au chargement du chunk ; disques et minerais de la même étape ne dépendent pas
des cartes vivantes). Écart au plan (tâches 2/3) assumé et à lever si un socle ultérieur
active une feature d'étape ≥ 7 lisant une carte vivante : il faudra alors un second chemin
`write_block` ordinaire mettant à jour les cartes de `TerrainFeatureWorld`.

### Pont terrain → région (`feature/bridge.rs`, interface du contrat)
`TerrainFeatureWorld` définit dans l'interface du crate le pont que les critères figés
fer/minerais présupposent, levant la cause « contrat » :
- `load_chunk` appelle `WorldGenerator::with_options(seed, Noise, carvers=true)` →
  `fill_chunk` (aquifères + carvers + matériaux, pas de structures) puis convertit chaque
  indice de la table `material().blocks` en identifiant d'ÉTAT par défaut
  (`default_state_id`) stocké dans `states` (indexé comme `ChunkTerrain::block_id_at`).
- Cartes `_WG` reconstruites FIGÉES à partir de la colonne d'états post-terrain (avant
  décoration) : `WORLD_SURFACE_WG` = plus haut `!air` + 1 ; `OCEAN_FLOOR_WG` = plus haut
  `blocks_motion` + 1 (proxy `has_collision`, exact pour les blocs de terrain pleins ;
  eau/lave/air/neige-couche/powder_snow ne bloquent pas) ; `MOTION_BLOCKING(_NO_LEAVES)` =
  plus haut `blocks_motion || liquid` + 1 (pas de feuilles avant décoration).
- Biomes stockés : `material().biome_source.fill_chunk` par chunk ; `stored_biome(cx,cy,cz)`
  indexe la grille par `(cell_y+16)·16 + (cell_z&3)·4 + (cell_x&3)`, cohérent avec le zoom
  flou `feature/zoom.rs` (même algorithme que `material::biome_at`, même `zoom_seed(seed)`).
- `TerrainRegion` applique D1–D5 : lecture réelle d'un chunk chargé jusqu'à `d = 8`, air si
  non chargé à `d ≥ 2`, `void_air` hors `[−64,319]`, biome en erreur à `d ≥ 2`, écriture
  `d ≤ 1` dans le monde partagé mutable, refus silencieux `d = 2..8`, `Err` à `d ≥ 9`.
- Comparaison : `block_id_at` reconvertit l'état courant en identifiant de bloc
  (`block_id_of_state`) pour confronter l'oracle (registre `minecraft:block`).

### H-ordre et H-bande (D6) — harnais oracle livrés
`tests/oracle_features/fer.rs` et `tests/oracle_features/minerais.rs` (pilote commun
`tests/oracle_features/common.rs`, un seul binaire de test) décorent un MONDE PARTAGÉ mutable. Les 84 cibles forment le POURTOUR
d'un carré 22×22 (cx, cz ∈ [−11, 10]). Ensemble décoré = les cibles ∪ leur anneau de rayon 1,
BORNÉ au carré englobant des cibles : les voisins extérieurs (cx = −12/11, cz = −12/11) sont
chargés pour les lectures mais JAMAIS décorés. Ensemble chargé = ensemble décoré ∪ son anneau.
Preuve par l'oracle (H-bord) : avec l'anneau extérieur décoré, la porte fer relève 106 écarts,
tous sur la colonne de bord extérieur des cibles (x = 0 de cx = −11, z = 15 de cz = 10, z = 0
de cz = −11…), et toujours dans le même sens : Verdigris pose `iron_ore`/`deepslate_iron_ore`/
`clay` là où l'oracle garde pierre, deepslate ou eau. Le monde de référence n'a donc reçu aucun
débordement de ces voisins : ils n'ont pas atteint l'étape de décoration (seuls les chunks
listés, statut `initialize_light`, sont décorés). Borner l'ensemble décoré ramène ces écarts
à 0. Ne pas rétablir l'anneau extérieur complet : il rend l'égalité fer impossible.
H-ordre : `cz` croissant puis `cx` croissant ; chaque chunk décoré avec ses 8 voisins via
`Decoration::decorate_chunk_filtered` et le filtre `enabled_placed_features` de `index.json`.
Le débordement d'un voisin dans une cible persiste dans le monde partagé (c'est la source de
l'ambiguïté d'ordre que la bande D6 absorbe). Le fer est EXACT malgré l'ordre : `ore_iron` a
`discard = 0` (aucune lecture d'exposition) et écrit toujours `iron_ore`/`deepslate_iron_ore`
sur pierre/deepslate, valeur idempotente, indépendante de l'ordre ; de même `disk_clay`.
H-bande (D6) : une case `(lx,lz)` d'une cible est « dans la bande » si elle est à moins de 8
blocs d'un bord partagé avec un voisin DÉCORÉ (`in_band`, par bord réellement partagé, jamais
d'OU global élargi au pourtour de la porte). Le bord extérieur d'une cible du pourtour n'est
donc pas dans la bande (voisin non décoré) : l'égalité y est exigée exacte. Métriques
minerais : hors bande, égalité exacte exigée ; sur l'union des cases modifiées par vanilla OU
Verdigris, ≥ 99 % identiques (taux global + paires attendu/obtenu au diagnostic). Test de
non-régression de la bande : `band_leaves_interior_when_neighbor_undecorated`. INCERTAIN :
l'ordre vanilla exact de génération des chunks n'est pas spécifié par l'oracle ; H-ordre est
l'hypothèse retenue, sans effet sur le fer (idempotent) et absorbé par la bande pour les
minerais.

### Correction carver : herbe remise sous un creux noyé
`carver/apply.rs` : quand un carver a traversé un bloc déclencheur de surface (herbe,
mycélium) et creuse au-dessus d'un bloc de `dirt`, ce `dirt` reçoit le matériau de surface
QUEL QUE SOIT l'état posé (air, eau ou lave). L'ancienne garde `cat == Category::Air` le
limitait à l'air. Preuve par l'oracle : 3 cases du chunk (10, −2) (x = 1 y = 60 z = 7,
x = 2 y = 60 z = 8, x = 0 y = 61 z = 6) valent `grass_block` sous de l'eau dans le monde de
référence, `dirt` avec la garde. Aucune feature du filtre fer ne pose d'herbe : l'écart venait
du terrain. `oracle_carvers`, `oracle_materiaux`, `oracle` et `oracle_1b` restent verts après
la correction.

### Budget de la porte cargo-test (600 s)
La borne de l'ensemble décoré réduit aussi l'ensemble chargé par test (moins de chunks
générés qu'avec l'anneau complet). Pas de cache de grilles de biomes : un cache par
`(cx, cz)` dans `ChunkTerrain` ne serait jamais purgé côté serveur (croissance avec chaque
chunk généré). Les deux tests oracle tournent dans la porte par défaut, sans `#[ignore]`.
Ils partagent un seul binaire (`tests/oracle_features/main.rs`) : cargo enchaîne les binaires
de test l'un après l'autre mais lance les tests d'un même binaire en parallèle, ce qui superpose
les deux générations (environ 5 min chacune en débogage) au lieu de les additionner.

## Cache de grilles de biomes (perf/cache-biomes)

Le poste d'optimisation annoncé plus haut (« cache des grilles voisines côté serveur »)
est désormais réalisé, mais porté par `BiomeSource` lui-même et non par `ChunkTerrain` :
un champ `BiomeGridCache` unique partagé par les trois lecteurs (`MaterialRules::apply`,
`feature/bridge.rs::load_chunk`, `NoiseGenerator::generate`), tous passant déjà par
`generator.material().biome_source`. La seconde source construite côté serveur a été
supprimée pour que le partage soit effectif. Capacité par défaut 4096, configurable via
`BiomeSource::with_cache_capacity` / `WorldgenData::build_biome_source_with_capacity`.

Éviction FIFO : `HashMap<(i32,i32), Arc<[u16]>>` plus une `VecDeque` d'ordre d'insertion ;
une clé n'est poussée qu'à sa première insertion (un second écrivain écrase la valeur sans
re-pousser), donc la file ne contient jamais de clé périmée et la borne reste exacte. Les
grilles évincées survivent via leurs `Arc` déjà clonés. Le verrou (`Mutex`, pas de sharding)
n'est tenu que le temps d'un `get`/`insert`, jamais pendant le calcul d'une grille.

`state.reset()` reste le premier effet observable sur `SearchState`, sur chaque chemin de
`fill_chunk` comme de `grid`, hit ou miss : le cache n'est consulté qu'après. L'invariant
d'ordre de parcours est isolé dans `fill_grid_computed` (corps historique sans le reset),
réutilisé par les deux. Les six portes oracle restent vertes sans toucher leurs attentes.

Mesures (informatives, jamais une porte). Avant (fourni) : `WorldGenerator::fill_chunk`
≈ 414 ms/chunk, `BiomeSource::fill_chunk` ≈ 48 ms, mesuré le 2026-11-05, graine 20261029,
machine 20 cœurs, build release. Après : la grille centrale produite par `apply` est relue
en hit par `generate`, et le voisinage 3×3 partage ses bords d'un chunk au suivant ; le
coût mesurable est reporté par `bench_ordered_zone_ms_per_chunk` (`#[ignore]` par défaut,
`cargo test -p verdigris-worldgen --test biome_cache -- --ignored --nocapture`).

Les deux chemins chauds de décoration ont par ailleurs été revus : aucune boucle non bornée
(les features activées n'emploient que `count`/`in_square`/`height_range`/`heightmap`/`biome`/
`rarity_filter`/`block_predicate_filter` ; `ore`/`scattered_ore`/`disk` bornent toutes leurs
boucles). `features_fer_match_oracle` et `features_minerais_match_oracle` tournent donc dans la
porte par défaut (`cargo test --workspace`), aux côtés de `band_leaves_interior_…`, des
vecteurs `tests/features_vectors.rs` (§11.1/§11.2, dont le tirage/décalages de `scattered_ore`
§9.3) et de `tests/biome_block.rs`/`tests/allocation.rs` (région mémoire `TestRegion`).
