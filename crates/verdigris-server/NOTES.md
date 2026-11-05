# Notes du serveur Verdigris

## Commande /give

- Verdigris n'a pas d'entité objet au sol ni de son de ramassage. Le surplus d'un
  don qui ne tient pas dans les 36 cases de l'inventaire est donc perdu
  silencieusement, contrairement au serveur vanilla qui ferait tomber l'objet au sol.
- Le plafond de refus est `max_stack_size x 100` (par exemple 1600 pour une perle de
  l'Ender dont la pile vaut 16). Au-delà, aucun inventaire n'est modifié et aucun
  paquet d'inventaire n'est émis.
- Les tailles de pile proviennent uniquement de `item_props.<nom>.max_stack_size`
  dans `blocks.json`, préchargées une fois. Aucune valeur n'est codée en dur.

## Points INCERTAIN du composant objet

- La couleur de rareté est toujours `white` faute de données de rareté dans
  `blocks.json`. Un objet épique ou légendaire devrait afficher une autre couleur.
- Les clés de traduction des objets spéciaux (bannières, potions à effets, têtes)
  ne sont pas différenciées : seule la distinction bloc (`block.minecraft.*`) vs
  objet (`item.minecraft.*`) est gérée.

## Numérotation de l'arbre de commandes

- Le noeud `give` et ses trois arguments sont ajoutés en fin de liste de noeuds
  (append-only), ce qui diverge de la renumérotation BFS stricte de vanilla mais
  préserve l'invariance octet pour octet des sous-arbres des commandes existantes.
  La conformité réelle côté client est validée par le scénario d'intégration TCP
  Alice/Bob qui exécute `/give` avec un flux client complet.

## Ordonnancement de la génération des chunks

- Le cache de `World` (verdigris-protocol) stocke un slot `OnceLock` par coordonnée : le
  verrou global n'est tenu que pour obtenir ou insérer le slot, la génération se fait hors
  verrou, et deux demandes concurrentes du même chunk ne déclenchent qu'une génération.
- Borne de concurrence DANS `World` (`with_generation_limit`, compteur sous `Mutex` et
  `Condvar`) : seul le thread qui initialise un slot prend un permis, pendant le calcul. Le
  serveur fixe `max(1, available_parallelism - 1)` permis.
- Écarté après mesure : un sémaphore async pris dans la tâche du joueur avant
  `spawn_blocking`. Les joueurs qui attendaient le même chunk gardaient chacun un permis,
  et la boucle d'envoi attendait un permis avant d'envoyer des chunks déjà prêts : 64
  générations en 30 s, 6613 chunks reçus (régression).
- Écarté après mesure : quatre demandes d'avance par joueur. Inutile une fois la génération
  hors verrou (le débit vient du parallélisme entre joueurs), et défavorable au monde plat.
- Piste suivante, mesurée mais non livrée : construire la trame compressée dans la tâche
  bloquante plutôt que sur les workers async. En noise, 31 811 chunks reçus au lieu de
  23 451 ; en plat, premier chunk plus lent faute de borne sur ce travail (jusqu'à 100
  threads bloquants pour 20 cœurs). À reprendre avec une borne d'encodage.

### Mesure (2026-11-05, release, 20 cœurs, verdigris-bots : 100 bots à 50/s, rayon 8, marche 4 blocs/s, 30 s)

| générateur | variante | premier chunk p50 | p95 | chunks reçus |
|---|---|---|---|---|
| noise | avant (cache de biomes seul) | 211 ms | 556 ms | 22 455 |
| noise | après | 11 ms | 474 ms | 23 451 |
| flat | avant (3 mesures) | 36 à 59 ms | 116 à 242 ms | 31 900 |
| flat | après (3 mesures) | 46 à 91 ms | 205 à 355 ms | 31 900 |

## Décoration côté serveur (générateur noise)

Le générateur `noise` passe par `DecoratedGenerator` : chaque chunk émis contient le
terrain puis les décorations (minerais, argile, gravier, sable) dans les états
protocole. Le générateur `flat` reste inchangé.

### Séquence d'émission d'un chunk C

1. Sous verrou, liste des prototypes manquants du carré 5x5 centré sur C
   (`!is_loaded && !emitted`), puis verrou relâché.
2. Hors verrou, remplissage du terrain (brouillon `thread_local`) et construction de
   chaque prototype manquant.
3. Un seul tenu de verrou, jamais pendant un remplissage : insertion des prototypes
   construits (double garde), décoration du carré 3x3 centré sur C dans l'ordre `cz`
   croissant puis `cx` croissant, copie des états et biomes de C (état 0 = air),
   marquage de C émis, puis libération des prototypes devenus inutiles.

### Modèle d'états et correctif de concurrence

- Un seul `Mutex`, aucune `Condvar` ni attente par chunk. La décoration est
  sérialisée sous verrou ; seul le remplissage de terrain est parallèle, hors verrou.
  Deux ensembles sous verrou : `decorated` (chunks déjà décorés) et `emitted` (chunks
  déjà copiés dans une colonne protocole). `emitted` ne retire jamais son marqueur,
  même après libération du prototype (anti-résurrection).
- Invariant `emitted ⟹ decorated` : un chunk n'est émis que comme centre C d'un
  `generate`, dont l'étape de décoration couvre le 3x3 incluant C. Tout chunk émis est
  donc déjà décoré.
- Garde d'insertion `!is_loaded && !emitted`, évaluée hors verrou pour choisir quoi
  construire PUIS réévaluée sous verrou juste avant l'insertion. Un prototype construit
  hors verrou n'est jamais inséré après avoir été émis puis libéré par un autre thread.
- Non-perte d'écritures à distance 0–1 : une décoration de N n'écrit qu'à distance ≤ 1
  (spec 09 §1.2, `WRITE_RADIUS = 1`). Une cible d'écriture P d'une décoration de N est
  un voisin de N, donc dans le 5x5 de C. Si P avait été libéré, ses 8 voisins (dont N)
  seraient décorés ; or N n'est décoré (sauté) que s'il est déjà dans `decorated`, ce
  qui contredit « N en cours de décoration ». Donc P est présent. Aucune attente requise.
- Un voisin requis n'est jamais retiré : `remove_chunk(P)` n'a lieu que si P est émis ET
  ses 8 voisins décorés. Si N est en cours de décoration (absent de `decorated`), aucun
  de ses voisins (dont P) n'a pu être retiré, car ce retrait aurait exigé N décoré.
- Pas de double décoration : la boucle de décoration saute tout N déjà dans `decorated`.
  L'ensemble étant idempotent, un chunk décoré par un thread n'est jamais redécoré par un
  autre, sans réveil ni réévaluation d'attente à surveiller.

### Hors périmètre

- Persistance des prototypes : un chunk non émis est régénéré depuis le terrain après
  redémarrage ; l'ordre de décoration des bords peut alors différer (analogue à
  l'ambiguïté H-ordre de l'oracle). Aucun prototype n'est conservé sur disque.
- Structures, arbres et features non implémentées (`Unimplemented`) ne posent rien.
- Aucun calcul d'éclairage nouveau n'est introduit par la décoration.
- D9 : une lecture de bloc ou de hauteur à distance 2–8 d'un chunk absent renvoie de
  l'air / une valeur vide. Accepté : ce cas est non déterministe côté vanilla et
  n'entraîne aucune rétention de prototype.
- Les erreurs de remplissage de terrain insèrent un prototype d'air (biomes normaux)
  traité comme un succès, en parité avec le chunk d'air déjà renvoyé auparavant. Une
  erreur de décoration est journalisée, conserve les poses déjà faites, inscrit le
  chunk dans `decorated` et laisse la génération se poursuivre. La règle de libération
  s'applique sans exception : aucune transaction ni retour arrière.

### Correspondance d'états worldgen → protocole (D5)

- Le worldgen et le protocole dérivent leurs identifiants d'état du même
  `blocks.json` (26.3). Un état worldgen est donc identique à l'état protocole de même
  bloc : la traduction `wg_to_proto` est l'identité, et `emit_column` copie les états du
  prototype directement dans la colonne (état 0 = air). Aucun remappage explicite.
- Le test `worldgen_protocol_state_correspondence` vérifie pour chaque bloc matériel que
  `default_state_id` (worldgen) égale `default_state_of_name` (protocole). Toute
  divergence future devrait être codée explicitement et listée ici.

### Banc D8 (2026-11-05, même protocole : noise, 100 bots à 50/s, rayon 8, 30 s, 20 cœurs)

| variante | premier chunk p50 | p95 | chunks reçus |
|---|---|---|---|
| sans décoration (master) | 9 à 11 ms | 474 à 500 ms | ~23 600 |
| décoration, terrains du 5x5 remplis en séquence | 2566 ms | 4265 ms | 12 867 |
| décoration, terrains réservés et remplis en parallèle | 12 ms | 790 à 895 ms | 17 900 à 18 200 |

- Remplissage du 5x5 : sous le verrou, une génération réserve (`building`) les terrains qu'elle
  calcule ; ceux réservés par une autre génération sont attendus (`Condvar`), jamais recalculés.
  Les terrains réservés sont remplis en parallèle hors verrou (`thread::scope`, au plus
  `TERRAIN_WORKERS = 8` threads). Un garde `Claim` libère les réservations si un remplissage
  panique, pour qu'aucune génération n'attende indéfiniment.
- Coût restant de la décoration (environ -25 % de chunks reçus, p95 sous la seconde) : 25
  terrains par chunk au démarrage et un verrou de décoration unique. Piste suivante si besoin :
  décorer hors du verrou global les chunks dont les 3x3 ne se recouvrent pas.
- Les quatre tests coûteux de `decoration.rs` sont `#[ignore]` en debug et ne tournent ni dans
  la porte ni en CI : `cargo test --release -p verdigris-server decoration -- --include-ignored`.
