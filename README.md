# Verdigris

Serveur Minecraft Java Edition écrit en Rust. Seul le serveur change : les joueurs se connectent avec le client Java vanilla, sans mod.

Version visée : **Minecraft 26.3**, protocole **777**.

## État

Un client 26.3 vanilla :

- voit le serveur dans sa liste multijoueur (MOTD, version, joueurs, ping) ;
- se connecte en mode offline, avec compression zlib ;
- reçoit les 32 registres synchronisés et les tags, puis entre en play ;
- apparaît sur un monde plat, reçoit ses chunks autour de lui et les voit se charger ou s'oublier quand il se déplace ;
- casse et pose des blocs à portée, avec resynchronisation si le geste est refusé ;
- utilise la molette (`pick_item_from_block`, 0x24) : l'objet correspondant à l'état du bloc visé est recherché dans l'inventaire principal suivi (36 cases, index 0 à 8 pour la barre, 9 à 35 pour la réserve) ; selon le cas la case choisie change, une occurrence en réserve est échangée, ou l'objet est ajouté en créatif, puis le serveur émet les `container_set_slot` (0x14) utiles dans l'ordre croissant des index avec un compteur d'état par joueur, suivis d'un `set_held_slot` (0x6B) ;
- met à jour son inventaire en créatif via `set_creative_mode_slot` pour les cases menu 9 à 44.

Hors périmètre pour le choix de bloc : le booléen `include_data` et les entités de bloc (lus puis ignorés), `pick_item_from_entity`, l'inventaire de survie côté serveur, l'armure et la main secondaire, les menus et conteneurs. Plus largement : le mode online (chiffrement et authentification), les entités et les plugins ne sont pas implémentés.

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
| `crates/verdigris-server` | Binaire `verdigris` : réseau tokio, une tâche par connexion, machine à états du protocole |
| `crates/verdigris-protocol/data/26.3/network.json` | Registres synchronisés et tags de la 26.3 |
| `crates/verdigris-protocol/data/26.3/worldgen.json` | Réglages de bruit de l'overworld, fonctions de densité et bruits de la 26.3 |
| `tools/datagen.py` | Génère `network.json`, `blocks.json` et `worldgen.json` à partir du générateur de données officiel |
| `tools/oracle.py` | Construit le datapack de l'oracle et exporte ses chunks en fixtures de test |
| `tests-data/oracle-1a` | Relief vanilla de référence : 40 chunks, solide ou non solide bloc par bloc |

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
