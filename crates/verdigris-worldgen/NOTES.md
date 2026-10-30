# Notes — évaluation en mode volume des fonctions de densité

## Produit scalaire du bruit de gradient (H1)
L'ordre §1.5 `((g.x·u)+(g.z·w))+(g.y·v)` n'est pas réimplémenté pour le mode
volume : la forme volume (02 §2.2) ne change que le calcul des coordonnées
`b·(σ·ν)` (voir `octaves::sample_volume`), le bruit de gradient par nœud réutilise
l'ordre de produit scalaire déjà présent côté point (§1.4). §1.5 est déclaré
bit-identique sauf signe d'un zéro, sans effet sur les vecteurs validés. À
confronter à l'oracle si un écart apparaît.

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
