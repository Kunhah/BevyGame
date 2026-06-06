#!/usr/bin/env python3
"""Generate a human-readable catalog of every ability from AbilitiesExample.ron.

Parses the RON by hand (regex/line scan — the format is regular) and emits a
markdown reference grouped by owner (the seven protagonists), then by magic
school for the shared spell pools, then enemy/demo entries. Re-run after editing
abilities to refresh docs/abilities.md.
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "assets/data/abilities/AbilitiesExample.ron")
OUT = os.path.join(ROOT, "docs/abilities.md")

# id ranges that belong to a named protagonist (everything else groups by school)
CHAR_RANGES = [
    ("Rina — Rogue / kunoichi", range(20480, 20494)),
    ("Sayaka — Cleric / Kitsune", range(22528, 22539)),
    ("Houjou Utaka — Samurai", range(24576, 24589)),
    ("Toshiko — Vessel", range(26624, 26636)),
    ("Renjiro — Monk (sōhei/yamabushi)", list(range(28672, 28678)) + list(range(28704, 28710))),
    ("Suzuka — Onmyoji", list(range(28680, 28686)) + list(range(28712, 28718))),
    ("Kanzo — Exorcist (biwa-hōshi)", list(range(28688, 28694)) + list(range(28720, 28726))),
]
DEMO_IDS = {0, 2050, 4097, 6146}
YOKAI_IDS = set(range(30720, 30723))
SCHOOL_GROUP = {
    "Kiho": "Kiho — shared spells (inner force)",
    "Onmyodo": "Onmyodo — shared spells (gogyō / seals)",
    "Kamishin": "Kamishin — shared spells (kami / liturgy)",
    "Yokaijutsu": "Yokaijutsu — shared spells (bargains)",
}
GROUP_ORDER = [g for g, _ in CHAR_RANGES] + list(SCHOOL_GROUP.values()) + [
    "Enemy / yokai abilities",
    "Demo / examples",
]


def group_for(aid, school):
    for name, ids in CHAR_RANGES:
        if aid in ids:
            return name
    if aid in DEMO_IDS:
        return "Demo / examples"
    if aid in YOKAI_IDS:
        return "Enemy / yokai abilities"
    return SCHOOL_GROUP.get(school, "Other")


def status_name(kind):
    m = re.match(r"\w+\((\w+)\)", kind)
    return m.group(1) if m else kind


def balanced_end(block, open_paren):
    """Index of the ')' matching the '(' at open_paren."""
    depth, j = 0, open_paren
    while j < len(block):
        if block[j] == "(":
            depth += 1
        elif block[j] == ")":
            depth -= 1
            if depth == 0:
                return j
        j += 1
    return len(block) - 1


def summarise_effects(block):
    out = []
    pat = re.compile(r"(Damage|Heal|Buff|ApplyStatus|RemoveStatus|Summon)\(")
    pos = 0
    while True:
        m = pat.search(block, pos)
        if not m:
            break
        kind = m.group(1)
        open_paren = m.end() - 1
        end = balanced_end(block, open_paren)
        body = block[open_paren + 1:end]
        pos = end + 1  # skip the whole effect so nested calls aren't re-matched
        f = dict(re.findall(r"(\w+):\s*([^,]+?(?:\([^)]*\))?)(?:,|$)", body))
        if kind == "Damage":
            out.append(f"Damage {f.get('floor','?')}-{f.get('ceiling','?')} {f.get('damage_type','?')}")
        elif kind == "Heal":
            out.append(f"Heal {f.get('floor','?')}-{f.get('ceiling','?')}")
        elif kind == "Buff":
            out.append(f"Buff {f.get('stat','?')} ×{f.get('multiplier','?')}")
        elif kind == "ApplyStatus":
            km = re.search(r"kind:\s*(\w+\(\w+\))", body)
            tm = re.search(r"tier:\s*(\d+)", body)
            out.append(f"{status_name(km.group(1)) if km else '?'} t{tm.group(1) if tm else '?'}")
        elif kind == "RemoveStatus":
            km = re.search(r"kind:\s*(\w+\(\w+\))", body)
            out.append(f"Cleanse {status_name(km.group(1)) if km else '?'}")
        elif kind == "Summon":
            sm = re.search(r"kind:\s*(\w+)", body)
            lm = re.search(r"lifetime_turns:\s*(\d+)", body)
            out.append(f"Summon {sm.group(1) if sm else '?'} ({lm.group(1) if lm else '?'}t)")
    return ", ".join(out) if out else "—"


def parse(text):
    abilities = []
    cur = None
    in_effects = False
    eff = ""
    for line in text.splitlines():
        m = re.match(r"\s*id:\s*(\d+),", line)
        if m:
            if cur:
                cur["effects"] = summarise_effects(eff)
                abilities.append(cur)
            cur = {"id": int(m.group(1))}
            in_effects, eff = False, ""
            continue
        if cur is None:
            continue
        if re.match(r"\s*effects:\s*\[", line):
            in_effects = True
            eff = line
            if "]" in line:
                in_effects = False
            continue
        if in_effects:
            eff += "\n" + line
            if "]" in line:
                in_effects = False
            continue
        for key in ("name", "description"):
            mm = re.match(rf'\s*{key}:\s*"(.*)",\s*$', line)
            if mm:
                cur[key] = mm.group(1)
        for key in ("magic_school", "shape"):
            mm = re.match(rf"\s*{key}:\s*(.+?),\s*$", line)
            if mm:
                cur[key] = mm.group(1)
        for key in ("magic_cost", "action_point_cost", "cooldown", "targets"):
            mm = re.match(rf"\s*{key}:\s*([\d.]+),", line)
            if mm:
                cur[key] = mm.group(1)
    if cur:
        cur["effects"] = summarise_effects(eff)
        abilities.append(cur)
    return abilities


def main():
    with open(SRC, encoding="utf-8") as fh:
        abilities = parse(fh.read())
    groups = {}
    for a in abilities:
        g = group_for(a["id"], a.get("magic_school", ""))
        groups.setdefault(g, []).append(a)

    lines = [
        "# Ability Catalog",
        "",
        f"_Auto-generated from `assets/data/abilities/AbilitiesExample.ron` by "
        f"`scripts/gen_ability_catalog.py` — {len(abilities)} abilities. Re-run the "
        "script after editing abilities._",
        "",
    ]
    for g in GROUP_ORDER + [k for k in groups if k not in GROUP_ORDER]:
        items = groups.get(g)
        if not items:
            continue
        lines.append(f"## {g}")
        lines.append("")
        for a in sorted(items, key=lambda x: x["id"]):
            ap = a.get("action_point_cost", "?")
            cd = a.get("cooldown", "?")
            mc = a.get("magic_cost", "0")
            cost = f"{ap} AP"
            if mc not in ("0", "0.0"):
                cost += f", {mc} {a.get('magic_school','')}"
            if cd not in ("0",):
                cost += f", CD {cd}"
            tgt = a.get("targets", "1")
            shape = a.get("shape", "Select")
            area = "" if shape == "Select" else f" · {shape}"
            lines.append(
                f"- **{a.get('name','?')}** `0x{a['id']:04X}` · "
                f"{a.get('magic_school','—')} · {cost} · "
                f"{a.get('effects','—')}{area} · {tgt} target(s)"
            )
            lines.append(f"  {a.get('description','')}")
        lines.append("")

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))
    print(f"Wrote {OUT} ({len(abilities)} abilities, {len(groups)} groups)")


if __name__ == "__main__":
    main()
