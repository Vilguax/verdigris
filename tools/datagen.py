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
    gameplay_out = Path(out).parent / "gameplay.json"
    gameplay = gameplay_doc(generated, server_jar)
    gameplay_out.write_text(json.dumps(gameplay, separators=(",", ":"), ensure_ascii=False) + NEWLINE, encoding="utf-8")
    print(f"{gameplay_out}: {len(gameplay['damage_type'])} types de degats, {len(gameplay['food'])} aliments")
    features_out = Path(out).parent / "features.json"
    features = features_doc(data / "worldgen", server_jar)
    features_out.write_text(json.dumps(features, separators=(",", ":"), ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{features_out}: {len(features['placed_feature'])} placed features, {len(features['feature'])} features, "
          f"{len(features['biome_features'])} biomes")
    print(f"{worldgen_out}: {len(worldgen['density_function'])} fonctions de densite, "
          f"{len(worldgen['noise'])} bruits, reglages {sorted(worldgen['noise_settings'])}, "
          f"{len(worldgen['biome_parameters']['entries']['minecraft:overworld'])} points de biomes, "
          f"{len(worldgen['material_rule'])} regles de materiaux, {len(worldgen['biome'])} biomes")
    crafting_out = Path(out).parent / "crafting.json"
    crafting = crafting_doc(generated, static, server_jar)
    crafting_out.write_text(json.dumps(crafting, separators=(",", ":"), ensure_ascii=False) + NEWLINE, encoding="utf-8")
    recipes = crafting["recipes"]
    print(f"{crafting_out}: {len(recipes)} recettes, "
          f"{sum(r['type'] in CRAFTING_MATCHED for r in recipes)} de fabrication, "
          f"{sum(r['type'] == 'minecraft:stonecutting' for r in recipes)} de taille de pierre, "
          f"{sum(len(r.get('displays', [])) for r in recipes)} displays, {len(crafting['fuels'])} combustibles, "
          f"{len(crafting['craft_remainders'])} restes, {len(crafting['property_sets'])} ensembles de proprietes "
          f"{ {k: len(v) for k, v in crafting['property_sets'].items()} }, {len(crafting['unlocks'])} deblocages")


WORLDGEN_SETTINGS = ["overworld"]


NEWLINE = chr(10)


def gameplay_doc(generated, server_jar):
    data = Path(generated) / "data" / "minecraft"
    damage = {"minecraft:" + p.stem: json.loads(p.read_text(encoding="utf-8"))
              for p in sorted((data / "damage_type").glob("*.json"))}
    tags = {"minecraft:" + p.relative_to(data / "tags" / "damage_type").with_suffix("").as_posix():
            json.loads(p.read_text(encoding="utf-8"))["values"]
            for p in sorted((data / "tags" / "damage_type").rglob("*.json"))}
    components = Path(generated) / "reports" / "minecraft" / "components" / "item"
    food = {}
    for p in sorted(components.glob("*.json")):
        body = json.loads(p.read_text(encoding="utf-8"))["components"]
        if "minecraft:food" in body:
            food["minecraft:" + p.stem] = {k: body[k] for k in ("minecraft:food", "minecraft:consumable",
                                                               "minecraft:use_remainder") if k in body}
    return {"game_version": GAME_VERSION, "server_jar_sha1": hashlib.sha1(Path(server_jar).read_bytes()).hexdigest(),
            "damage_type": damage, "damage_type_tags": tags, "food": food}


CRAFT_REMAINDERS = {
    "minecraft:water_bucket": "minecraft:bucket", "minecraft:lava_bucket": "minecraft:bucket",
    "minecraft:milk_bucket": "minecraft:bucket", "minecraft:honey_bottle": "minecraft:glass_bottle",
    "minecraft:dragon_breath": "minecraft:glass_bottle",
}
CRAFTING_MATCHED = {"minecraft:crafting_shaped", "minecraft:crafting_shapeless", "minecraft:crafting_transmute"}
CRAFTING_BOOK = {"building": "crafting_building_blocks", "redstone": "crafting_redstone",
                 "equipment": "crafting_equipment", "misc": "crafting_misc"}
BOOK_CATEGORY = {
    "minecraft:crafting_shaped": CRAFTING_BOOK, "minecraft:crafting_shapeless": CRAFTING_BOOK,
    "minecraft:crafting_transmute": CRAFTING_BOOK, "minecraft:crafting_dye": CRAFTING_BOOK,
    "minecraft:crafting_imbue": CRAFTING_BOOK,
    "minecraft:smelting": {"blocks": "furnace_blocks", "food": "furnace_food", "misc": "furnace_misc"},
    "minecraft:blasting": {"blocks": "blast_furnace_blocks", "food": "blast_furnace_misc", "misc": "blast_furnace_misc"},
    "minecraft:smoking": {"blocks": "smoker_food", "food": "smoker_food", "misc": "smoker_food"},
    "minecraft:campfire_cooking": {"blocks": "campfire", "food": "campfire", "misc": "campfire"},
    "minecraft:stonecutting": {"misc": "stonecutter"},
    "minecraft:smithing_transform": {"misc": "smithing"}, "minecraft:smithing_trim": {"misc": "smithing"},
}
COOKING_INPUT = {"minecraft:smelting": "minecraft:furnace_input", "minecraft:blasting": "minecraft:blast_furnace_input",
                 "minecraft:smoking": "minecraft:smoker_input", "minecraft:campfire_cooking": "minecraft:campfire_input"}
SMITHING_SLOTS = ["template", "base", "addition"]


def crafting_doc(generated, static, server_jar):
    data = Path(generated) / "data" / "minecraft"
    item_ids = {k: v["protocol_id"] for k, v in static["minecraft:item"]["entries"].items()}
    book_ids = {k: v["protocol_id"] for k, v in static["minecraft:recipe_book_category"]["entries"].items()}
    tag_root = data / "tags" / "item"
    raw_tags = {"minecraft:" + p.relative_to(tag_root).with_suffix("").as_posix():
                json.loads(p.read_text(encoding="utf-8"))["values"] for p in tag_root.rglob("*.json")}

    def item(where, name):
        if name not in item_ids:
            raise SystemExit(f"{where}: objet inconnu {name}")
        return item_ids[name]

    def ingredient(where, value):
        if isinstance(value, str) and value.startswith("#"):
            if value[1:] not in raw_tags:
                raise SystemExit(f"{where}: tag inconnu {value}")
            return {"tag": value[1:]}
        names = [value] if isinstance(value, str) else value
        if not names:
            raise SystemExit(f"{where}: ingredient vide")
        return {"items": [item(where, n) for n in names]}

    def resolved(value):
        if "tag" in value:
            return flatten("minecraft:item", value["tag"], raw_tags, item_ids)
        return value["items"]

    def stack(where, result):
        return {"item": item(where, result["id"]), "count": result.get("count", 1),
                "has_components": "components" in result}

    def shaped(rid, d):
        pattern = d["pattern"]
        if not 1 <= len(pattern) <= 3 or len({len(r) for r in pattern}) != 1 or not 1 <= len(pattern[0]) <= 3:
            raise SystemExit(f"{rid}: motif invalide {pattern}")
        if set("".join(pattern)) - {" "} != set(d["key"]) or not d["key"]:
            raise SystemExit(f"{rid}: symboles du motif et de la cle differents")
        filled = [(x, y) for y, row in enumerate(pattern) for x, ch in enumerate(row) if ch != " "]
        x0, x1 = min(x for x, _ in filled), max(x for x, _ in filled)
        y0, y1 = min(y for _, y in filled), max(y for _, y in filled)
        key = {k: ingredient(rid, v) for k, v in d["key"].items()}
        ingredients, cells = [], []
        for row in pattern[y0:y1 + 1]:
            for ch in row[x0:x1 + 1]:
                if ch != " " and key[ch] not in ingredients:
                    ingredients.append(key[ch])
                cells.append(-1 if ch == " " else ingredients.index(key[ch]))
        width, height = x1 - x0 + 1, y1 - y0 + 1
        symmetric = all(cells[y * width + x] == cells[y * width + width - 1 - x]
                        for y in range(height) for x in range(width))
        return {"width": width, "height": height, "ingredients": ingredients, "cells": cells,
                "symmetric": symmetric, "result": stack(rid, d["result"])}

    def transmute(rid, d):
        count = d.get("material_count", 1)
        low, high = (count, count) if isinstance(count, int) else (count["min"], count["max"])
        if not 1 <= low <= high <= 8:
            raise SystemExit(f"{rid}: nombre de materiaux invalide {count}")
        if "components" in d["result"]:
            raise SystemExit(f"{rid}: resultat a composants")
        out = {"input": ingredient(rid, d["input"]), "material": ingredient(rid, d["material"]), "min": low, "max": high,
               "add_material_count": d.get("add_material_count_to_result", False)}
        if "id" in d["result"]:
            out["result_item"] = item(rid, d["result"]["id"])
        out["count"] = d["result"].get("count", 1)
        return out

    def fields(rid, d):
        kind = d["type"]
        if kind == "minecraft:crafting_shaped":
            return shaped(rid, d)
        if kind == "minecraft:crafting_shapeless":
            if not 1 <= len(d["ingredients"]) <= 9:
                raise SystemExit(f"{rid}: {len(d['ingredients'])} ingredients")
            return {"ingredients": [ingredient(rid, i) for i in d["ingredients"]], "result": stack(rid, d["result"])}
        if kind == "minecraft:crafting_transmute":
            return transmute(rid, d)
        if kind in COOKING_INPUT:
            return {"ingredient": ingredient(rid, d["ingredient"]), "result": stack(rid, d["result"]),
                    "experience": d.get("experience", 0), "cookingtime": d["cookingtime"]}
        if kind == "minecraft:stonecutting":
            return {"ingredient": ingredient(rid, d["ingredient"]), "result": stack(rid, d["result"])}
        if kind in ("minecraft:smithing_transform", "minecraft:smithing_trim"):
            return {s: ingredient(rid, d[s]) for s in SMITHING_SLOTS}
        if kind == "minecraft:brewing":
            return {"input": item(rid, d["input"]["item"]), "reagent": item(rid, d["reagent"]["item"])}
        return {}

    recipe_root = data / "recipe"
    sources = {"minecraft:" + p.relative_to(recipe_root).with_suffix("").as_posix():
               json.loads(p.read_text(encoding="utf-8")) for p in recipe_root.rglob("*.json")}
    recipes, groups, display_count = [], {}, 0
    property_sets = {k: set() for k in [*COOKING_INPUT.values(), "minecraft:smithing_template", "minecraft:smithing_base",
                                        "minecraft:smithing_addition", "minecraft:brewing_input",
                                        "minecraft:brewing_reagent"]}
    for rid in sorted(sources):
        d = sources[rid]
        kind = d["type"]
        entry, extra = {"id": rid, "type": kind}, fields(rid, d)
        if d.get("group"):
            groups.setdefault(d["group"], len(groups))
        if kind in BOOK_CATEGORY:
            count = extra["max"] - extra["min"] + 1 if kind == "minecraft:crafting_transmute" else 1
            entry["category"] = book_ids["minecraft:" + BOOK_CATEGORY[kind][d.get("category", "misc")]]
            if d.get("group"):
                entry["group"] = groups[d["group"]]
            entry["show_notification"] = d.get("show_notification", True)
            entry["displays"] = list(range(display_count, display_count + count))
            display_count += count
        entry.update(extra)
        recipes.append(entry)
        if kind in COOKING_INPUT:
            property_sets[COOKING_INPUT[kind]].update(resolved(entry["ingredient"]))
        elif kind in ("minecraft:smithing_transform", "minecraft:smithing_trim"):
            for s in SMITHING_SLOTS:
                property_sets[f"minecraft:smithing_{s}"].update(resolved(entry[s]))
        elif kind == "minecraft:brewing":
            property_sets["minecraft:brewing_input"].add(entry["input"])
            property_sets["minecraft:brewing_reagent"].add(entry["reagent"])

    providers = data / "context_int_provider"

    def provider(ref):
        return json.loads((providers / (ref.split(":", 1)[1] + ".json")).read_text(encoding="utf-8"))

    components = Path(generated) / "reports" / "minecraft" / "components" / "item"
    fuels = []
    for name in sorted(item_ids, key=item_ids.get):
        body = json.loads((components / f"{name.split(':', 1)[1]}.json").read_text(encoding="utf-8"))["components"]
        fuel = body.get("minecraft:cooking_fuel")
        if fuel is None:
            continue
        burn = provider(fuel["burn_time"])
        right = burn.get("right", {})
        if (burn.get("type") != "minecraft:div" or not isinstance(burn.get("left"), int)
                or right.get("type") != "minecraft:conditional" or right.get("condition") != "minecraft:block/fast_cooking"
                or fuel["speed_multiplier"] != "minecraft:cooking/speed_default"):
            raise SystemExit(f"combustible {name}: forme inattendue {fuel} {burn}")
        normal, fast = provider(right["on_false"]), provider(right["on_true"])
        if not isinstance(normal, int) or not isinstance(fast, int) or normal <= 0 or fast <= 0:
            raise SystemExit(f"combustible {name}: diviseurs inattendus {normal} {fast}")
        fuels.append({"item": item_ids[name], "burn_normal": burn["left"] // normal, "burn_fast": burn["left"] // fast})

    unlocks, unlocked = [], set()
    advancement_root = data / "advancement" / "recipes"
    for p in sorted(advancement_root.rglob("*.json")):
        body = json.loads(p.read_text(encoding="utf-8"))
        if "rewards" not in body:
            continue
        where = "avancement " + p.relative_to(advancement_root).with_suffix("").as_posix()
        [recipe] = body["rewards"]["recipes"]
        if recipe not in sources or recipe in unlocked:
            raise SystemExit(f"{where}: recette {recipe} inconnue ou deja debloquee")
        if len(body["requirements"]) != 1 or sorted(body["requirements"][0]) != sorted(body["criteria"]):
            raise SystemExit(f"{where}: exigences non disjonctives")
        unlocked.add(recipe)
        entry = {"recipe": recipe, "items": []}
        for criterion in body["criteria"].values():
            trigger, cond = criterion["trigger"], criterion.get("conditions", {})
            if trigger == "minecraft:recipe_unlocked" and cond == {"recipes": recipe}:
                continue
            if (trigger == "minecraft:inventory_changed" and list(cond) == ["items"] and len(cond["items"]) == 1
                    and list(cond["items"][0]) == ["items"]):
                entry["items"].append(ingredient(where, cond["items"][0]["items"]))
            elif (trigger == "minecraft:inventory_changed" and list(cond) == ["slots"]
                  and list(cond["slots"]) == ["occupied"] and list(cond["slots"]["occupied"]) == ["min"]):
                entry["slots_min"] = cond["slots"]["occupied"]["min"]
            elif trigger == "minecraft:enter_block" and cond == {"blocks": "minecraft:water"}:
                entry["enter_water"] = True
            elif trigger == "minecraft:tick" and not cond:
                entry["tick"] = True
            else:
                raise SystemExit(f"{where}: critere non gere {criterion}")
        unlocks.append(entry)
    unlocks.sort(key=lambda u: u["recipe"])
    remainders = sorted(({"item": item("reste", k), "remainder": item("reste", v)} for k, v in CRAFT_REMAINDERS.items()),
                        key=lambda r: r["item"])

    return {"game_version": GAME_VERSION, "server_jar_sha1": hashlib.sha1(Path(server_jar).read_bytes()).hexdigest(),
            "recipes": recipes, "fuels": fuels, "craft_remainders": remainders,
            "property_sets": {k: sorted(v) for k, v in sorted(property_sets.items())},
            "unlocks": unlocks}


def features_doc(worldgen, server_jar):
    def folder(sub):
        root = worldgen / sub
        return {"minecraft:" + p.relative_to(root).with_suffix("").as_posix(): json.loads(p.read_text(encoding="utf-8"))
                for p in sorted(root.rglob("*.json"))}
    biomes = folder("biome")
    return {"game_version": GAME_VERSION, "server_jar_sha1": hashlib.sha1(Path(server_jar).read_bytes()).hexdigest(),
            "placed_feature": folder("placed_feature"), "feature": folder("feature"),
            "block_state_provider": folder("block_state_provider"),
            "biome_features": {k: v.get("features", []) for k, v in biomes.items()}}


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
