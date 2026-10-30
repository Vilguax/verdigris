# Verdigris

Serveur Minecraft Java Edition écrit en Rust. Seul le serveur change : les joueurs se connectent avec le client Java vanilla, sans mod.

Version visée : **Minecraft 26.3**, protocole **777**.

## État

Un client 26.3 vanilla :

- voit le serveur dans sa liste multijoueur (MOTD, version, joueurs, ping) ;
- se connecte en mode offline, avec compression zlib ;
- reçoit les 32 registres synchronisés et les tags, puis entre en play ;
- apparaît sur un monde au relief vanilla (ou plat selon la configuration), reçoit ses chunks autour de lui et les voit se charger ou s'oublier quand il se déplace ;
- casse et pose des blocs à portée, avec resynchronisation si le geste est refusé ;
- utilise la molette (`pick_item_from_block`, 0x24) : l'objet correspondant à l'état du bloc visé est recherché dans l'inventaire principal suivi (36 cases, index 0 à 8 pour la barre, 9 à 35 pour la réserve) ; selon le cas la case choisie change, une occurrence en réserve est échangée, ou l'objet est ajouté en créatif, puis le serveur émet les `container_set_slot` (0x14) utiles dans l'ordre croissant des index avec un compteur d'état par joueur, suivis d'un `set_held_slot` (0x6B) ;
- met à jour son inventaire en créatif via `set_creative_mode_slot` pour les cases menu 9 à 44 ;
- voit la liste des joueurs (touche Tab) : à l'entrée, chaque joueur reçoit une mise à jour unique contenant tous les joueurs présents, et les joueurs déjà connectés reçoivent l'ajout du nouveau ; à chaque départ (propre ou interrompu), les joueurs restants reçoivent la suppression du partant. L'en-tête affiche le MOTD, le pied `joueurs_en_ligne/maximum`. La latence mesurée par le keep alive est diffusée à tous toutes les 30 secondes, et un changement de mode de jeu est diffusé immédiatement avec l'action correspondante seule.

Hors périmètre pour le choix de bloc : le booléen `include_data` et les entités de bloc (lus puis ignorés), `pick_item_from_entity`, l'inventaire de survie côté serveur, l'armure et la main secondaire, les menus et conteneurs. Plus largement : le mode online (chiffrement et authentification), les entités et les plugins ne sont pas implémentés.

Hors périmètre de la liste Tab : les entités des autres joueurs dans le monde, les skins et propriétés signées, le chat et son initialisation, le nom d'affichage personnalisé.

## Lancer

```sh
cargo run --release
```

Le serveur écoute sur `0.0.0.0:25565`. Dans le client, ajoutez le serveur `127.0.0.1` (préférez l'IPv4 à `localhost`).

Les logs se règlent avec `VERDIGRIS_LOG`, en syntaxe `EnvFilter` de `tracing` :

```sh
VERDIGRIS_LOG=debug cargo run --release
```

Le niveau par défaut est `info`. `debug` liste chaque paquet reçu et émis.

## Génération du monde

Deux générateurs sont disponibles, choisis au démarrage par variables d'environnement :

- `VERDIGRIS_GENERATOR` : `noise` (défaut) pour le relief vanilla 26.3, ou `flat` pour l'ancien monde plat historique. Toute autre valeur refuse le démarrage.
- `VERDIGRIS_SEED` : graine entière i64 (défaut `20261029`). Une valeur non entière refuse le démarrage.

```sh
VERDIGRIS_GENERATOR=noise VERDIGRIS_SEED=20261029 cargo run --release
```

En mode `noise`, un générateur est construit une seule fois par graine et partagé entre toutes les tâches ; `final_density` est évaluée une seule fois en mode volume sur 16 × 384 × 16 blocs par chunk. Un bloc est pierre si la densité finale est strictement positive, sinon lave sous `y = −54`, eau sous `y = 63`, air au-dessus. Le serveur ajoute uniquement la bedrock à `y = −64`, puis dérive heightmaps et lumière du ciel. Le point d'apparition se place au-dessus du plus haut bloc solide de la colonne `(0, 0)`. La génération d'un chunk s'exécute hors des tâches réseau (`spawn_blocking`) et le temps moyen de génération par chunk est journalisé toutes les 30 secondes.

Au premier démarrage d'un dossier de monde, un fichier `level.json` (`{ generator, seed, game_version }`) est écrit à la racine du monde. Aux démarrages suivants, le générateur et la graine demandés doivent correspondre à ceux stockés, sinon le serveur refuse de démarrer avec un message indiquant la valeur stockée et la valeur demandée. `game_version` est informatif et ne bloque pas.

## Persistance des chunks

Les chunks modifiés (blocs posés ou cassés) sont sauvegardés sur disque dans un format maison versionné, puis rechargés à l'identique au redémarrage.

- Répertoire du monde : variable `VERDIGRIS_WORLD` (défaut `world`). Les chunks vivent dans `VERDIGRIS_WORLD/chunks/<x>.<z>.bin`.
- Au premier besoin d'un chunk, le serveur le charge depuis son fichier s'il existe, sinon il le génère.
- Seuls les chunks effectivement modifiés sont écrits. L'écriture a lieu au plus tard toutes les 30 secondes, et systématiquement lors d'un arrêt propre (`Ctrl+C`) avant la fin du processus. Chaque passage journalise le nombre de chunks écrits.
- Chaque écriture remplace le fichier cible de façon atomique (fichier temporaire puis renommage) et ne laisse aucun fichier temporaire après succès.
- Format maison : en-tête clair (magique `VRDG`, version de format, version de jeu) puis corps zlib de sections palettisées auto-descriptives. Un fichier illisible (mauvaise version, en-tête ou corps corrompu) est renommé en `<x>.<z>.bin.rejete` et le chunk est régénéré.

Hors périmètre : le format région vanilla `.mca`, les joueurs, les inventaires et les entités ne sont pas persistés.

## Structure

| Chemin | Rôle |
|---|---|
| `crates/verdigris-protocol` | Codec et paquets, sans aucune entrée/sortie : VarInt, chaînes, trames compressées, login, configuration |
| `crates/verdigris-server` | Binaire `verdigris` : réseau tokio, une tâche par connexion, machine à états du protocole, sélection du générateur et `level.json` |
| `crates/verdigris-bots` | Binaire `verdigris-bots` : générateur de charge, un faux client 26.3 par bot |
| `crates/verdigris-worldgen` | Génération de relief pure (sans E/S ni tokio) : bruits, fonctions de densité, remplissage de chunk en catégories pierre/eau/lave/air |
| `crates/verdigris-protocol/data/26.3/network.json` | Registres synchronisés et tags de la 26.3 |
| `crates/verdigris-protocol/data/26.3/worldgen.json` | Réglages de bruit de l'overworld, fonctions de densité, bruits, règles de matériaux, climat des biomes et points climatiques des biomes (rapport `biome_parameters`, une ligne par point dans l'ordre température, humidité, continentalité, érosion, profondeur, bizarrerie, décalage) de la 26.3 |
| `tools/datagen.py` | Génère `network.json`, `blocks.json` et `worldgen.json` à partir du générateur de données officiel |
| `tools/oracle.py` | Construit le datapack de l'oracle et exporte ses chunks en fixtures de test |
| `tests-data/oracle-1a` | Relief vanilla de référence : 40 chunks, solide ou non solide bloc par bloc |
| `tests-data/oracle-1b` | Relief et aquifères vanilla : 64 chunks, air, solide, eau ou lave bloc par bloc |
| `tests-data/oracle-biomes` | Biomes vanilla : 64 chunks couvrant 21 biomes, un identifiant réseau par cellule de 4 × 4 × 4 blocs |

## Données du jeu

`network.json` contient les identifiants de registres et les tags du jeu. Il est produit par `tools/datagen.py` à partir de la sortie du générateur de données du `server.jar` officiel :

```sh
java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --server --reports --output generated
python tools/datagen.py generated server.jar crates/verdigris-protocol/data/26.3/network.json
```

Le `server.jar` n'est pas inclus dans ce dépôt. Le générateur de la 26.3 demande Java 25.

## Oracle du relief

Le relief généré par Verdigris est comparé à celui d'un serveur vanilla 26.3 sur la même graine (`20261029`). Pour isoler la forme du terrain, le monde de référence est créé avec un datapack qui retire les aquifères, les carvers et les features, et sans structures :

```sh
python tools/oracle.py datapack server.jar <monde>/datapacks/oracle-1a
```

Dans ce monde, un bloc est solide exactement quand la densité finale est positive. Les règles de matériaux remplacent la pierre par d'autres blocs solides sans changer cette frontière. Seule la glace posée au niveau de l'eau est ambiguë : elle est exclue de la comparaison.

Les chunks générés sont ensuite exportés en fixtures, un octet par bloc (0 non solide, 1 solide, 2 exclu), compressé en zlib, indexé par `(y + 64) * 256 + z * 16 + x` :

```sh
python tools/oracle.py export <monde>/dimensions/minecraft/overworld/region tests-data/oracle-1a 0,0 1,0
```

`tests-data/oracle-1a/index.json` décrit le format et liste les chunks.

La phase des aquifères a son propre oracle, `tests-data/oracle-1b` : même graine, datapack qui garde les aquifères (`--aquiferes`) et retire toujours carvers, features et structures, export à cinq valeurs par bloc (0 air, 1 solide, 2 exclu, 3 eau, 4 lave, option `--fluides`). Il compte 64 chunks : les 40 de `oracle-1a` et 24 choisis pour leur eau souterraine et leur lave d'aquifère.

```sh
python tools/oracle.py datapack server.jar <monde>/datapacks/oracle-1b --aquiferes
python tools/oracle.py export <monde>/dimensions/minecraft/overworld/region tests-data/oracle-1b 0,0 1,0 --fluides
```

Les biomes ont leur oracle, `tests-data/oracle-biomes`, tiré du même monde : un octet par cellule de 4 × 4 × 4 blocs, l'identifiant réseau du biome (ordre du registre `minecraft:worldgen/biome` de `network.json`), indexé par `(qy + 16) * 16 + qz * 4 + qx`. Les 64 chunks sont choisis pour couvrir les 21 biomes présents autour du point d'apparition.

```sh
python tools/oracle.py export-biomes <monde>/dimensions/minecraft/overworld/region tests-data/oracle-biomes 0,0 1,0
```

La porte oracle est le test d'intégration `final_fill_matches_oracle_on_all_chunks` du crate `verdigris-worldgen`. Elle construit le générateur une fois, remplit les 40 chunks et exige l'égalité exacte (solide ⇔ pierre) sur toutes les cases non exclues, avec diagnostic de chunk, position et densité en cas d'écart :

```sh
cargo test -p verdigris-worldgen --test oracle
```

## Mesure de charge

Le binaire `verdigris-bots` ouvre un grand nombre de faux clients Java 26.3 (protocole 777) pour mesurer la tenue en charge d'un serveur. Chaque bot se connecte en **mode hors ligne** (UUID dérivé du pseudo, compression zlib), traverse login puis configuration, entre en jeu, répond aux keep-alive, ping et téléportations, accuse réception des lots de chunks et émet un tick toutes les 50 ms. Les données de chunks reçues sont comptées puis **jetées** : rien n'est conservé en mémoire.

Le générateur ne dépend d'aucune particularité de Verdigris : il n'utilise que les identifiants et formats de la 26.3, et fonctionne donc aussi contre un serveur **vanilla** ou **Paper** 26.3 configuré en mode hors ligne (`online-mode=false`). Le mode hors ligne des deux côtés est la condition d'une comparaison équitable.

```sh
cargo run --release -p verdigris-bots -- \
  --count 200 --rate 10 --duration 60 --walk 4 --view-distance 8 --json report.json
```

Le même appel vise un serveur vanilla ou Paper hors ligne en précisant `--host` et `--port` :

```sh
cargo run --release -p verdigris-bots -- \
  --host 127.0.0.1 --port 25565 --count 200 --rate 10 --duration 60 --json report.json
```

Options :

- `--host` (défaut `127.0.0.1`), `--port` (défaut `25565`) ;
- `--count N` : nombre de bots (erreur si `0`) ;
- `--rate R` : bots lancés par seconde (erreur si non fini ou `<= 0`), intervalle de lancement `1/R` ;
- `--duration S` : durée de mesure en secondes (défaut `60`), **décomptée à partir du dernier lancement** ;
- `--prefix` (défaut `Bot`) : les pseudos sont `Bot0001`, `Bot0002`, … (quatre chiffres, index à partir de 1) ;
- `--view-distance` (défaut `8`), `--walk D` : déplacement aléatoire de `D * 0,05` bloc par tick dans le plan XZ (défaut `0`, erreur si négatif ou non fini) ;
- `--json <chemin>` : écrit le rapport final en JSON (écriture atomique).

La fenêtre de mesure démarre après la montée en charge : `measure_deadline = fin de la montée + duration`. À cette échéance l'arrêt global est déclenché ; `Ctrl+C` le déclenche aussi immédiatement, et tous les bots se terminent proprement. Un bot encore en jeu à l'arrêt est compté « en jeu » (ce n'est pas un échec).

Délais par défaut (en dur) qui produisent la cause d'échec `Timeout` : connexion TCP 10 s, login 15 s, configuration 15 s, première position en jeu 20 s, silence réseau en jeu 30 s.

Toutes les 5 secondes, un rapport intermédiaire affiche les bots en jeu, les échecs, les chunks par seconde et le débit. Le rapport final (texte et JSON) donne les bots demandés, en jeu et en échec ventilés par cause, les raisons de déconnexion lisibles, les percentiles p50/p95/p99 et le maximum (avec leur taille d'échantillon `n`) des durées jusqu'au login terminé, à l'entrée en jeu et au premier chunk, ainsi que les chunks et octets reçus.

Hors périmètre : le mode online (chiffrement, authentification), le chat, l'édition du monde, le rendu, et la mesure de la charge CPU ou mémoire côté serveur.

Un test de conformité rejoue une séquence login → configuration → jeu au format 26.3 sans socket réel et vérifie les réponses de la machine à états, indépendamment de Verdigris. Un test d'intégration optionnel, activé par `VERDIGRIS_BOTS_EXTERNAL_PORT`, lance 20 bots contre un serveur vanilla ou Paper hors ligne déjà démarré sur ce port :

```sh
VERDIGRIS_BOTS_EXTERNAL_PORT=25565 cargo test -p verdigris-bots -- --ignored external_offline
```

## Vérifications

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

La CI GitHub Actions exécute ces trois commandes à chaque push et pull request.

## Licence

Au choix, [MIT](LICENSE-MIT) ou [Apache 2.0](LICENSE-APACHE).

Verdigris n'est ni affilié à Mojang ni approuvé par Mojang ou Microsoft. Minecraft est une marque de Mojang Synergies AB.
