import hashlib
import json
import sys
from pathlib import Path

GAME_VERSION = "26.3"
PROTOCOL = 777

SYNCED = [
    "worldgen/biome", "chat_type", "trim_pattern", "trim_material", "wolf_variant",
    "wolf_sound_variant", "pig_variant", "pig_sound_variant", "frog_variant", "cat_variant",
    "cat_sound_variant", "cow_sound_variant", "cow_variant", "chicken_sound_variant",
    "chicken_variant", "zombie_nautilus_variant", "painting_variant", "sulfur_cube_archetype",
    "dimension_type", "damage_type", "banner_pattern", "enchantment", "jukebox_song", "instrument",
    "test_environment", "test_instance", "dialog", "world_clock", "timeline",
    "decorated_pot_pattern", "block_transformer", "block_state_provider",
]
MAY_BE_EMPTY = {"block_state_provider"}


def entries_of(folder):
    return sorted("minecraft:" + p.relative_to(folder).with_suffix("").as_posix()
                  for p in folder.rglob("*.json"))


def flatten(registry, name, raw_tags, ids, stack=()):
    if name in stack:
        raise SystemExit(f"cycle de tags {registry} {' -> '.join(stack + (name,))}")
    out = []
    for value in raw_tags[name]:
        required = True
        if isinstance(value, dict):
            required = value.get("required", True)
            value = value["id"]
        if value.startswith("#"):
            ref = value[1:]
            if ref not in raw_tags:
                if required:
                    raise SystemExit(f"tag {registry} {name}: reference absente {value}")
                continue
            out += flatten(registry, ref, raw_tags, ids, stack + (name,))
        elif value in ids:
            out.append(ids[value])
        elif required:
            raise SystemExit(f"tag {registry} {name}: entree absente {value}")
    return list(dict.fromkeys(out))


def main(generated, server_jar, out):
    generated, data = Path(generated), Path(generated) / "data" / "minecraft"
    static = json.loads((generated / "reports" / "registries.json").read_text(encoding="utf-8"))

    registries, ids_by_registry = [], {}
    for reg in SYNCED:
        entries = entries_of(data / reg)
        if not entries and reg not in MAY_BE_EMPTY:
            raise SystemExit(f"registre synchronise vide: {reg}")
        registries.append({"id": f"minecraft:{reg}", "entries": entries})
        ids_by_registry[f"minecraft:{reg}"] = {e: i for i, e in enumerate(entries)}
    for reg, body in static.items():
        ids_by_registry.setdefault(reg, {e: v["protocol_id"] for e, v in body["entries"].items()})

    tags, skipped = [], []
    tag_root = data / "tags"
    folders = sorted({p.parent for p in tag_root.rglob("*.json")})
    roots = {}
    for folder in folders:
        rel = folder.relative_to(tag_root).as_posix()
        reg = next((f"minecraft:{rel[:len(r)]}" for r in sorted(
            {k.split(":", 1)[1] for k in ids_by_registry}, key=len, reverse=True)
            if rel == r or rel.startswith(r + "/")), None)
        if reg is None:
            skipped.append(rel)
            continue
        roots.setdefault(reg, tag_root / reg.split(":", 1)[1])
    for reg, root in sorted(roots.items()):
        raw = {"minecraft:" + p.relative_to(root).with_suffix("").as_posix():
               json.loads(p.read_text(encoding="utf-8"))["values"] for p in root.rglob("*.json")}
        resolved = [{"name": n, "ids": flatten(reg, n, raw, ids_by_registry[reg])} for n in sorted(raw)]
        tags.append({"registry": reg, "tags": resolved})

    doc = {
        "game_version": GAME_VERSION,
        "protocol": PROTOCOL,
        "source": {"server_jar_sha1": hashlib.sha1(Path(server_jar).read_bytes()).hexdigest(),
                   "generator": "net.minecraft.data.Main --server --reports"},
        "known_pack": {"namespace": "minecraft", "id": "core", "version": GAME_VERSION},
        "registries": registries,
        "tags": tags,
    }
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    Path(out).write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{out}: {len(registries)} registres, {sum(len(r['entries']) for r in registries)} entrees, "
          f"{len(tags)} registres de tags, {sum(len(t['tags']) for t in tags)} tags, ignores: {skipped}")

    blocks_out = Path(out).parent / "blocks.json"
    blocks = blocks_doc(generated, static, server_jar)
    blocks_out.write_text(json.dumps(blocks, separators=(",", ":"), ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{blocks_out}: {blocks['state_count']} etats, {len(blocks['blocks'])} blocs, "
          f"{len(blocks['fluid_states'])} etats avec fluide, {len(blocks['items'])} objets qui posent un bloc")

    worldgen_out = Path(out).parent / "worldgen.json"
    worldgen = worldgen_doc(data / "worldgen", server_jar, generated / "reports")
    worldgen_out.write_text(json.dumps(worldgen, separators=(",", ":"), ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{worldgen_out}: {len(worldgen['density_function'])} fonctions de densite, "
          f"{len(worldgen['noise'])} bruits, reglages {sorted(worldgen['noise_settings'])}, "
          f"{len(worldgen['biome_parameters']['entries']['minecraft:overworld'])} points de biomes, "
          f"{len(worldgen['material_rule'])} regles de materiaux, {len(worldgen['biome'])} biomes")


WORLDGEN_SETTINGS = ["overworld"]


CLIMATE_ORDER = ["temperature", "humidity", "continentalness", "erosion", "depth", "weirdness", "offset"]
BIOME_CLIMATE = ["has_precipitation", "temperature", "temperature_modifier", "downfall", "carvers"]


def worldgen_doc(worldgen, server_jar, reports):
    def folder(sub):
        root = worldgen / sub
        return {"minecraft:" + p.relative_to(root).with_suffix("").as_posix(): json.loads(p.read_text(encoding="utf-8"))
                for p in sorted(root.rglob("*.json"))}
    settings = folder("noise_settings")
    missing = [s for s in WORLDGEN_SETTINGS if f"minecraft:{s}" not in settings]
    if missing:
        raise SystemExit(f"reglages de bruit absents: {missing}")
    return {"game_version": GAME_VERSION, "server_jar_sha1": hashlib.sha1(Path(server_jar).read_bytes()).hexdigest(),
            "noise_settings": {f"minecraft:{s}": settings[f"minecraft:{s}"] for s in WORLDGEN_SETTINGS},
            "density_function": folder("density_function"), "noise": folder("noise"),
            "material_rule": folder("material_rule"),
            "material_condition": folder("material_condition"),
            "carver": folder("carver"),
            "biome": {k: {f: v[f] for f in BIOME_CLIMATE if f in v} for k, v in folder("biome").items()},
            "biome_parameters": {"dimensions": CLIMATE_ORDER, "entries": {f"minecraft:{s}": [
                [b["biome"]] + [b["parameters"][d] for d in CLIMATE_ORDER]
                for b in json.loads((reports / "biome_parameters" / "minecraft" / f"{s}.json")
                                    .read_text(encoding="utf-8"))["biomes"]]
                for s in WORLDGEN_SETTINGS}}}


ALWAYS_FLUID = {"minecraft:water", "minecraft:lava", "minecraft:bubble_column", "minecraft:kelp",
                "minecraft:kelp_plant", "minecraft:seagrass", "minecraft:tall_seagrass"}
AIR = {"minecraft:air", "minecraft:cave_air", "minecraft:void_air"}
ITEM_TO_BLOCK = {
    "minecraft:redstone": "minecraft:redstone_wire", "minecraft:string": "minecraft:tripwire",
    "minecraft:wheat_seeds": "minecraft:wheat", "minecraft:cocoa_beans": "minecraft:cocoa",
    "minecraft:carrot": "minecraft:carrots", "minecraft:potato": "minecraft:potatoes",
    "minecraft:torchflower_seeds": "minecraft:torchflower_crop", "minecraft:pitcher_pod": "minecraft:pitcher_crop",
    "minecraft:beetroot_seeds": "minecraft:beetroots", "minecraft:sweet_berries": "minecraft:sweet_berry_bush",
    "minecraft:glow_berries": "minecraft:cave_vines", "minecraft:pumpkin_seeds": "minecraft:pumpkin_stem",
    "minecraft:melon_seeds": "minecraft:melon_stem",
}
NOT_BLOCK_ITEMS = {"minecraft:air", "minecraft:wheat"}


def blocks_doc(generated, static, server_jar):
    report = json.loads((Path(generated) / "reports" / "blocks.json").read_text(encoding="utf-8"))
    block_ids = {k: v["protocol_id"] for k, v in static["minecraft:block"]["entries"].items()}
    item_ids = {k: v["protocol_id"] for k, v in static["minecraft:item"]["entries"].items()}
    blocks, fluid_states, defaults, total = [], [], {}, 0
    for name, body in report.items():
        ids = [s["id"] for s in body["states"]]
        first, count = min(ids), len(ids)
        if ids != list(range(first, first + count)):
            raise SystemExit(f"etats non contigus pour {name}")
        default = next(s["id"] for s in body["states"] if s.get("default"))
        defaults[name] = default
        for s in body["states"]:
            if name in ALWAYS_FLUID or s.get("properties", {}).get("waterlogged") == "true":
                fluid_states.append(s["id"])
        props = [{"name": k, "values": v} for k, v in sorted(body.get("properties", {}).items())]
        for index, s in enumerate(body["states"]):
            rest, expected = index, {}
            for prop in reversed(props):
                rest, digit = divmod(rest, len(prop["values"]))
                expected[prop["name"]] = prop["values"][digit]
            if expected != s.get("properties", {}):
                raise SystemExit(f"ordre des proprietes inattendu pour {name}, etat {s['id']}")
        blocks.append({"name": name, "id": block_ids[name], "first_state": first, "state_count": count,
                       "default_state": default, "air": name in AIR, "properties": props})
        total += count
    blocks.sort(key=lambda b: b["first_state"])
    items = []
    for item, item_id in sorted(item_ids.items(), key=lambda kv: kv[1]):
        if item in NOT_BLOCK_ITEMS:
            continue
        block = ITEM_TO_BLOCK.get(item, item)
        if block in defaults:
            items.append({"item": item_id, "state": defaults[block]})
    components = Path(generated) / "reports" / "minecraft" / "components" / "item"
    item_props = {}
    for item in sorted(item_ids, key=item_ids.get):
        body = json.loads((components / f"{item.split(':', 1)[1]}.json").read_text(encoding="utf-8"))["components"]
        item_props[item] = {"max_stack_size": body["minecraft:max_stack_size"],
                            "equip_slot": body.get("minecraft:equippable", {}).get("slot")}
    return {"game_version": GAME_VERSION, "server_jar_sha1": hashlib.sha1(Path(server_jar).read_bytes()).hexdigest(),
            "state_count": total, "blocks": blocks, "fluid_states": sorted(fluid_states), "items": items,
            "item_ids": dict(sorted(item_ids.items(), key=lambda kv: kv[1])), "item_props": item_props}


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit("usage: datagen.py <dossier generated> <server.jar> <sortie.json>")
    main(*sys.argv[1:])
