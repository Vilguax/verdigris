# Verdigris

Serveur Minecraft Java Edition écrit en Rust. Seul le serveur change : les joueurs se connectent avec le client Java vanilla, sans mod.

Version visée : **Minecraft 26.3**, protocole **777**.

## État

Un client 26.3 vanilla :

- voit le serveur dans sa liste multijoueur (MOTD, version, joueurs, ping) ;
- se connecte en mode offline, avec compression zlib ;
- reçoit les 32 registres synchronisés et les tags, puis entre en play ;
- est ensuite déconnecté proprement avec le message « le monde n'est pas encore disponible ».

Pas encore implémenté : le monde (chunks, spawn, entités), le mode online (chiffrement et authentification), les plugins.

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

## Structure

| Chemin | Rôle |
|---|---|
| `crates/verdigris-protocol` | Codec et paquets, sans aucune entrée/sortie : VarInt, chaînes, trames compressées, login, configuration |
| `crates/verdigris-server` | Binaire `verdigris` : réseau tokio, une tâche par connexion, machine à états du protocole |
| `crates/verdigris-protocol/data/26.3/network.json` | Registres synchronisés et tags de la 26.3 |
| `tools/datagen.py` | Génère `network.json` à partir du générateur de données officiel |

## Données du jeu

`network.json` contient les identifiants de registres et les tags du jeu. Il est produit par `tools/datagen.py` à partir de la sortie du générateur de données du `server.jar` officiel :

```sh
java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --server --reports --output generated
python tools/datagen.py generated server.jar crates/verdigris-protocol/data/26.3/network.json
```

Le `server.jar` n'est pas inclus dans ce dépôt. Le générateur de la 26.3 demande Java 25.

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
