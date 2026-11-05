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
