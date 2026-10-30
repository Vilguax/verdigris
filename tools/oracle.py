import json
import struct
import sys
import zipfile
import zlib
from pathlib import Path

GAME_VERSION = "26.3"
SEED = 20261029
MIN_Y, HEIGHT = -64, 384
PACK_FORMAT = [121, 0]
NON_SOLID = {"minecraft:air", "minecraft:cave_air", "minecraft:void_air", "minecraft:water", "minecraft:lava"}
EXCLUDED = {"minecraft:ice"}
GENERATED = {"minecraft:terrain", "minecraft:features", "minecraft:initialize_light", "minecraft:light",
             "minecraft:spawn", "minecraft:full"}
CELL_NON_SOLID, CELL_SOLID, CELL_EXCLUDED, CELL_WATER, CELL_LAVA = 0, 1, 2, 3, 4
FLUID_CELLS = {"minecraft:water": CELL_WATER, "minecraft:lava": CELL_LAVA}


def nbt(buf, i, tag):
    fixed = {1: ">b", 2: ">h", 3: ">i", 4: ">q", 5: ">f", 6: ">d"}
    if tag in fixed:
        n = struct.calcsize(fixed[tag])
        return struct.unpack(fixed[tag], buf[i:i + n])[0], i + n
    if tag == 7:
        n = struct.unpack(">i", buf[i:i + 4])[0]
        return buf[i + 4:i + 4 + n], i + 4 + n
    if tag == 8:
        n = struct.unpack(">H", buf[i:i + 2])[0]
        return buf[i + 2:i + 2 + n].decode("utf-8", "replace"), i + 2 + n
    if tag == 9:
        t, n = buf[i], struct.unpack(">i", buf[i + 1:i + 5])[0]
        i += 5
        out = []
        for _ in range(n):
            v, i = nbt(buf, i, t)
            out.append(v)
        return out, i
    if tag == 10:
        out = {}
        while buf[i] != 0:
            t = buf[i]
            n = struct.unpack(">H", buf[i + 1:i + 3])[0]
            name = buf[i + 3:i + 3 + n].decode("utf-8", "replace")
            out[name], i = nbt(buf, i + 3 + n, t)
        return out, i + 1
    if tag in (11, 12):
        fmt = ">i" if tag == 11 else ">q"
        size = struct.calcsize(fmt)
        n = struct.unpack(">i", buf[i:i + 4])[0]
        return list(struct.unpack(f">{n}{fmt[1]}", buf[i + 4:i + 4 + size * n])), i + 4 + size * n
    raise ValueError(f"tag NBT inconnu {tag}")


def region_chunks(region):
    d = Path(region).read_bytes()
    for k in range(1024):
        off = int.from_bytes(d[4 * k:4 * k + 3], "big")
        if not off:
            continue
        o = off * 4096
        length, comp = struct.unpack(">iB", d[o:o + 5])
        raw = d[o + 5:o + 4 + length]
        data = zlib.decompress(raw) if comp == 2 else raw
        root, _ = nbt(data, 3 + struct.unpack(">H", data[1:3])[0], 10)
        yield root


def palette_name(entry):
    if isinstance(entry, str):
        return entry
    return entry.get("Name", entry.get("", entry.get("id")))


def palette_level(entry):
    if isinstance(entry, str):
        return "0"
    props = entry.get("Properties", entry.get("properties", {}))
    return props.get("level", "0")


def section_blocks(sec, levels=False):
    bs = sec["block_states"]
    pal = [(palette_name(p), palette_level(p)) if levels else palette_name(p) for p in bs["palette"]]
    if len(pal) == 1:
        return [pal[0]] * 4096
    bits = max(4, (len(pal) - 1).bit_length())
    per = 64 // bits
    out = []
    for long in bs["data"]:
        long &= (1 << 64) - 1
        for j in range(per):
            if len(out) < 4096:
                out.append(pal[(long >> (j * bits)) & ((1 << bits) - 1)])
    return out


def cells_of(chunk, fluids=False):
    cells = bytearray([CELL_NON_SOLID]) * (HEIGHT * 256)
    for sec in chunk["sections"]:
        if "block_states" not in sec:
            continue
        base = (sec["Y"] * 16 - MIN_Y) * 256
        if base < 0 or base >= len(cells):
            continue
        for i, (name, level) in enumerate(section_blocks(sec, levels=True)):
            if name in EXCLUDED or (fluids and name in FLUID_CELLS and level != "0"):
                cells[base + i] = CELL_EXCLUDED
            elif fluids and name in FLUID_CELLS:
                cells[base + i] = FLUID_CELLS[name]
            elif name not in NON_SOLID:
                cells[base + i] = CELL_SOLID
    return cells


def datapack(server_jar, out, aquifers=False):
    with zipfile.ZipFile(server_jar) as outer:
        inner = next(n for n in outer.namelist() if n.startswith("META-INF/versions/") and n.endswith(".jar"))
        jar = zipfile.ZipFile(__import__("io").BytesIO(outer.read(inner)))
    root = Path(out)
    wg = root / "data" / "minecraft" / "worldgen"
    for sub in ("carver", "noise_settings", "biome"):
        (wg / sub).mkdir(parents=True, exist_ok=True)
    for name in jar.namelist():
        if not name.endswith(".json"):
            continue
        if name.startswith("data/minecraft/worldgen/carver/"):
            body = json.loads(jar.read(name))
            body["probability"] = 0.0
            (wg / "carver" / Path(name).name).write_text(json.dumps(body, indent=2), encoding="utf-8")
        elif name.startswith("data/minecraft/worldgen/biome/"):
            body = json.loads(jar.read(name))
            body["features"], body["carvers"] = [], []
            (wg / "biome" / Path(name).name).write_text(json.dumps(body, indent=2), encoding="utf-8")
    settings = json.loads(jar.read("data/minecraft/worldgen/noise_settings/overworld.json"))
    if not aquifers:
        settings.pop("aquifers", None)
    (wg / "noise_settings" / "overworld.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    phase = "1b: aquiferes, sans carvers ni features" if aquifers else "1a: sans aquiferes, carvers ni features"
    meta = {"pack": {"description": f"Verdigris oracle phase {phase}",
                     "min_format": PACK_FORMAT, "max_format": PACK_FORMAT}}
    (root / "pack.mcmeta").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(f"datapack ecrit dans {root}")


def export(region_dir, out_dir, wanted, fluids=False):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    found = {}
    for region in sorted(Path(region_dir).glob("r.*.mca")):
        for c in region_chunks(region):
            key = (c["xPos"], c["zPos"])
            if c.get("Status") in GENERATED and (not wanted or key in wanted):
                found[key] = c
    missing = sorted(set(wanted) - set(found)) if wanted else []
    if missing:
        raise SystemExit(f"chunks absents ou non generes: {missing}")
    index = []
    for (x, z), c in sorted(found.items()):
        cells = cells_of(c, fluids)
        name = f"{x}.{z}.bin"
        (out / name).write_bytes(zlib.compress(bytes(cells), 9))
        entry = {"x": x, "z": z, "file": name, "solid": cells.count(CELL_SOLID),
                 "excluded": cells.count(CELL_EXCLUDED)}
        if fluids:
            entry["water"], entry["lava"] = cells.count(CELL_WATER), cells.count(CELL_LAVA)
        index.append(entry)
    legend = {"0": "non solide", "1": "solide", "2": "exclu"}
    if fluids:
        legend = {"0": "air", "1": "solide", "2": "exclu", "3": "eau", "4": "lave"}
    meta = {"format": 2 if fluids else 1, "game_version": GAME_VERSION, "seed": SEED, "min_y": MIN_Y,
            "height": HEIGHT, "index": "(y - min_y) * 256 + z * 16 + x", "cells": legend, "chunks": index}
    (out / "index.json").write_text(json.dumps(meta, indent=1) + "\n", encoding="utf-8")
    print(f"{len(index)} chunks exportes vers {out}")


BIOME_STATUSES = GENERATED | {"minecraft:biomes", "minecraft:noise", "minecraft:surface", "minecraft:carvers"}


def biome_ids():
    network = Path(__file__).resolve().parent.parent / "crates/verdigris-protocol/data/26.3/network.json"
    registries = json.loads(network.read_text(encoding="utf-8"))["registries"]
    entries = next(r["entries"] for r in registries if r["id"] == "minecraft:worldgen/biome")
    return {name: i for i, name in enumerate(entries)}


def section_biomes(sec, ids):
    bs = sec["biomes"]
    pal = [ids[palette_name(p)] for p in bs["palette"]]
    if len(pal) == 1:
        return [pal[0]] * 64
    bits = (len(pal) - 1).bit_length()
    per = 64 // bits
    out = []
    for long in bs["data"]:
        long &= (1 << 64) - 1
        for j in range(per):
            if len(out) < 64:
                index = (long >> (j * bits)) & ((1 << bits) - 1)
                if index >= len(pal):
                    raise SystemExit(f"indice de palette de biome hors bornes: {index}")
                out.append(pal[index])
    return out


def export_biomes(region_dir, out_dir, wanted):
    ids = biome_ids()
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    found = {}
    for region in sorted(Path(region_dir).glob("r.*.mca")):
        for c in region_chunks(region):
            key = (c["xPos"], c["zPos"])
            if c.get("Status") in BIOME_STATUSES and (not wanted or key in wanted):
                found[key] = c
    missing = sorted(set(wanted) - set(found)) if wanted else []
    if missing:
        raise SystemExit(f"chunks absents ou sans biomes: {missing}")
    index = []
    for (x, z), c in sorted(found.items()):
        cells = bytearray(HEIGHT // 4 * 16)
        for sec in c["sections"]:
            if "biomes" not in sec:
                continue
            base = (sec["Y"] * 4 - MIN_Y // 4) * 16
            if 0 <= base < len(cells):
                cells[base:base + 64] = bytes(section_biomes(sec, ids))
        name = f"{x}.{z}.bin"
        (out / name).write_bytes(zlib.compress(bytes(cells), 9))
        index.append({"x": x, "z": z, "file": name, "distinct": len(set(cells))})
    meta = {"format": 1, "game_version": GAME_VERSION, "seed": SEED, "min_y": MIN_Y, "height": HEIGHT,
            "cell": 4, "index": "(qy - min_y / 4) * 16 + qz * 4 + qx",
            "value": "identifiant reseau du biome (ordre du registre minecraft:worldgen/biome de network.json)",
            "chunks": index}
    (out / "index.json").write_text(json.dumps(meta, indent=1) + "\n", encoding="utf-8")
    print(f"{len(index)} chunks exportes vers {out}")


def parse_chunks(args):
    return {tuple(int(v) for v in a.split(",")) for a in args}


if __name__ == "__main__":
    flags = {a for a in sys.argv[2:] if a.startswith("--")}
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) >= 3 and args[0] == "datapack":
        datapack(args[1], args[2], aquifers="--aquiferes" in flags)
    elif len(args) >= 3 and args[0] == "export-biomes":
        export_biomes(args[1], args[2], parse_chunks(args[3:]))
    elif len(args) >= 3 and args[0] == "export":
        export(args[1], args[2], parse_chunks(args[3:]), fluids="--fluides" in flags)
    else:
        raise SystemExit("usage: oracle.py datapack <server.jar> <dossier datapack> [--aquiferes]\n"
                         "       oracle.py export <dossier region> <dossier sortie> [cx,cz ...] [--fluides]\n"
                         "       oracle.py export-biomes <dossier region> <dossier sortie> [cx,cz ...]")
