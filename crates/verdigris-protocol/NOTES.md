# Notes du protocole Verdigris

## Codecs d'inventaire

- Les décodeurs `decode_container_click` et `decode_container_close` ne sont PAS
  raccordés à l'énumération `Serverbound` ni à `decode_play` : les paquets 0x12
  (Container Click) et 0x13 (Container Close) restent classés `PlayIgnored`. Le
  raccordement effectif (routage vers une logique de conteneur) est reporté à la
  tâche serveur, qui ajoutera les variantes et appellera ces décodeurs. C'est
  volontaire pour ne pas élargir le périmètre du crate protocole ni casser le
  `match` exhaustif de `verdigris-server`.
- Bornes de sécurité avant toute lecture d'entrée : `MAX_CHANGED_SLOTS = 128`
  pour la liste de cases modifiées d'un clic, `MAX_HASHED_COMPONENTS = 256` pour
  chacune des listes de composants ajoutés/retirés d'une `HashedStack`. Une
  longueur négative donne `PrefixedArrayLengthNegative`, une longueur au-delà de
  la borne donne `PrefixedArrayTooLong`, et ces deux erreurs tombent AVANT la
  lecture des entrées (donc jamais confondues avec un `UnexpectedEof`).
- Le `mode` d'un clic est lu en `VarInt` puis converti par `ClickType::from_raw`.
  Toute valeur hors de 0..=6 (y compris négative) retombe sur `Pickup` (stratégie
  ZERO), au lieu de rejeter le paquet. L'ordre et les doublons de la liste de
  cases modifiées sont préservés tels quels.

## Point INCERTAIN de `HashedStack::matches`

- Une `ItemStack::Item` dont le `count` est <= 0 est traitée comme une pile vide
  (équivalente à `ItemStack::Empty`). Une `HashedStack::Item` comparée à une pile
  vide renvoie donc `false`, et une `HashedStack::Empty` comparée à une telle pile
  renvoie `true`. INCERTAIN : la comparaison exacte d'un objet renseigné face à
  une case vide n'a pas de vecteur de capture de référence ; la règle retenue
  (objet vs vide => false, composants non vides => false) suit le modèle vanilla
  mais devra être recroisée si un oracle devient disponible.

## Équipement (`equip_slot`)

- `equip_slot(item_id)` lit `item_props.<nom>.equip_slot` dans `blocks.json`
  (26.3). Les valeurs `head`, `chest`, `legs`, `feet`, `offhand` donnent la
  catégorie `EquipSlot` correspondante ; `null`, `body` et `saddle` donnent
  `None` (non portable sur un emplacement d'armure/main secondaire du joueur).
  Toute autre chaîne provoque une panique au chargement nommant l'objet, pour
  détecter une donnée inattendue au lieu de l'ignorer.
- Histogramme attendu sur les 1658 objets de 26.3 : Head 16, Chest 8, Legs 7,
  Feet 7, Offhand 1, None 1619. Un id négatif ou hors table renvoie `None`.

## Registres statiques (`static_registries.json`)

- Les quatre registres `minecraft:menu` (25), `minecraft:block_entity_type`
  (49), `minecraft:entity_type` (161) et `minecraft:sound_event` (1991) ont été
  extraits manuellement de `target/reports/registries.json` le 2026-11-06, pour
  la version 26.3 du serveur (`server_jar_sha1`
  `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c`). La clé `default` du registre
  `entity_type` est ignorée : seul l'id de protocole de chaque entrée est retenu.
- Au chargement, chaque registre est transformé en table indexée par id. Si les
  ids ne forment pas exactement `0..n-1` (trou, doublon, ou id hors borne), le
  chargement panique en nommant le registre fautif. Les recherches par nom ou par
  id renvoient `None` pour une entrée, un id ou un registre inconnu, sans jamais
  paniquer à l'appel.

## Titre de l'écran (`open_screen_payload`)

- Le titre est encodé via `text::write_nbt`, sans réimplémentation du NBT. Un
  titre `Component::translate("container.chest")` produit le NBT anonyme
  `0a 08 0009 "translate" 000f "container.chest" 00`, conforme au vecteur de
  référence. Les cinq encodeurs n'écrivent jamais l'identifiant de paquet : ils
  rendent uniquement la charge utile, comme `container_set_slot_payload`.
