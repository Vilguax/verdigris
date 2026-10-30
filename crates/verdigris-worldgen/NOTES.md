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
