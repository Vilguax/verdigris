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

## Interpolation (§4.12) renvoyée à la tâche volume-interpolated
L'évaluation volume d'`interpolated` est hors périmètre : `eval_volume` renvoie
`VolumeError::InterpolatedUnsupported` dès qu'un nœud `interpolated` est présent
dans l'arbre, sans panique. Les vecteurs d'accumulation verticale de 03 §4.12
(q0/q1 `0.1/0.7`, `0.123/0.987`) ne sont pas testés ici ; ils relèvent de la
future tâche « interpolated en volume ».

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
