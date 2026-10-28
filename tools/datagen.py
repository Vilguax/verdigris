import hashlib
import json
import sys
from pathlib import Path

GAME_VERSION = "26.3"
PROTOCOL = 777

SYNCED = [
    "banner_pattern", "chat_type", "damage_type", "dialog", "dimension_type", "enchantment",
    "instrument", "jukebox_song", "painting_variant", "sulfur_cube_archetype", "test_environment",
    "test_instance", "timeline", "trim_material", "trim_pattern", "world_clock", "worldgen/biome",
    "cat_variant", "cat_sound_variant", "chicken_variant", "chicken_sound_variant", "cow_variant",
    "cow_sound_variant", "frog_variant", "pig_variant", "pig_sound_variant", "wolf_variant",
    "wolf_sound_variant", "zombie_nautilus_variant",
]


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
        if not entries:
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


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit("usage: datagen.py <dossier generated> <server.jar> <sortie.json>")
    main(*sys.argv[1:])
