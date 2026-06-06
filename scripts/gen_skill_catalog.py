#!/usr/bin/env python3
"""Generate a human-readable catalog of every skill-tree node.

Parses all of assets/data/skills/*.ron and emits docs/skill_trees.md, grouped by
tree, with each node's tier, cost, prerequisites (resolved to node names),
effect summary, and description. UnlockAbility effects are resolved to the
ability's name via AbilitiesExample.ron. Re-run after editing skill trees.
"""
import glob
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SKILLS = os.path.join(ROOT, "assets/data/skills")
ABILITIES = os.path.join(ROOT, "assets/data/abilities/AbilitiesExample.ron")
OUT = os.path.join(ROOT, "docs/skill_trees.md")

# Display order + heading for each tree key (the `tree:` field value).
TREE_ORDER = [
    ("Kiho", "Kiho — inner force (magic source)"),
    ("Onmyodo", "Onmyodo — gogyō technique (magic source)"),
    ("Yokaijutsu", "Yokaijutsu — bargains with the Other (magic source)"),
    ("Kamishin", "Kamishin — communion with revered spirits (magic source)"),
    ("Martial", "Martial — universal weapon fundamentals"),
    ("Survival", "Survival — universal"),
    ("Bound", "Bound — universal (the Contract)"),
    ("RinaRogue", "Rina — Rogue / kunoichi (class tree)"),
    ("SayakaCleric", "Sayaka — Cleric / Kitsune (class tree)"),
    ("HoujouSamurai", "Houjou Utaka — Samurai (class tree)"),
    ("ToshikoVessel", "Toshiko — Vessel (class tree)"),
    ("RenjiroMonk", "Renjiro — Monk (class tree)"),
    ("SuzukaOnmyoji", "Suzuka — Onmyoji (class tree)"),
    ("KanzoExorcist", "Kanzo — Exorcist (class tree)"),
]


def ability_names():
    names = {}
    with open(ABILITIES, encoding="utf-8") as fh:
        text = fh.read()
    cur = None
    for line in text.splitlines():
        m = re.match(r"\s*id:\s*(\d+),", line)
        if m:
            cur = int(m.group(1))
        nm = re.match(r'\s*name:\s*"(.*)",\s*$', line)
        if nm and cur is not None:
            names[cur] = nm.group(1)
            cur = None
    return names


ABIL = ability_names()


def balanced_end(block, open_paren):
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
    pat = re.compile(r"(StatBonus|MagicRegenBonus|UnlockAbility|MagicCostReduction|Trigger)\(")
    pos = 0
    while True:
        m = pat.search(block, pos)
        if not m:
            break
        kind = m.group(1)
        op = m.end() - 1
        end = balanced_end(block, op)
        body = block[op + 1:end]
        pos = end + 1
        f = dict(re.findall(r"(\w+):\s*([\w.]+)", body))
        if kind == "StatBonus":
            out.append(f"+{f.get('amount','?')} {f.get('target','?')}")
        elif kind == "MagicRegenBonus":
            out.append(f"+{f.get('amount','?')} {f.get('school','?')}/rest")
        elif kind == "MagicCostReduction":
            pct = f.get("percent", "0")
            try:
                pct = f"{round(float(pct) * 100)}%"
            except ValueError:
                pass
            out.append(f"-{pct} {f.get('school','?')} cost")
        elif kind == "UnlockAbility":
            aid = int(f.get("ability_id", "0"))
            out.append(f"**unlocks** {ABIL.get(aid, '?')} (0x{aid:04X})")
        elif kind == "Trigger":
            out.append(f"trigger #{f.get('trigger_id','?')}")
    return out


def parse_nodes(text):
    nodes = []
    cur = None
    in_eff = False
    eff = ""
    for line in text.splitlines():
        m = re.match(r"\s*id:\s*(\d+),", line)
        if m:
            if cur:
                cur["effects"] = summarise_effects(eff)
                nodes.append(cur)
            cur = {"id": int(m.group(1))}
            in_eff, eff = False, ""
            continue
        if cur is None:
            continue
        if re.match(r"\s*effects:\s*\[", line):
            in_eff = True
            eff = line
            if "]" in line:
                in_eff = False
            continue
        if in_eff:
            eff += "\n" + line
            if "]" in line:
                in_eff = False
            continue
        nm = re.match(r'\s*name:\s*"(.*)",\s*$', line)
        if nm:
            cur["name"] = nm.group(1)
        dm = re.match(r'\s*description:\s*"(.*)",\s*$', line)
        if dm:
            cur["description"] = dm.group(1)
        for key in ("tree",):
            km = re.match(rf"\s*{key}:\s*(\w+),", line)
            if km:
                cur[key] = km.group(1)
        for key in ("tier", "cost"):
            km = re.match(rf"\s*{key}:\s*(\d+),", line)
            if km:
                cur[key] = int(km.group(1))
        pm = re.match(r"\s*prerequisites:\s*\[(.*)\],", line)
        if pm:
            cur["prereqs"] = [int(x) for x in re.findall(r"\d+", pm.group(1))]
    if cur:
        cur["effects"] = summarise_effects(eff)
        nodes.append(cur)
    return nodes


def main():
    all_nodes = []
    for path in glob.glob(os.path.join(SKILLS, "*.ron")):
        with open(path, encoding="utf-8") as fh:
            all_nodes.extend(parse_nodes(fh.read()))
    by_id = {n["id"]: n.get("name", "?") for n in all_nodes}
    by_tree = {}
    for n in all_nodes:
        by_tree.setdefault(n.get("tree", "?"), []).append(n)

    lines = [
        "# Skill-Tree Catalog",
        "",
        f"_Auto-generated from `assets/data/skills/*.ron` by "
        f"`scripts/gen_skill_catalog.py` — {len(all_nodes)} nodes across "
        f"{len(by_tree)} trees. Passive effects apply once, permanently, when a "
        "node is learned (see `skill_tree::apply_skill_effect`). Re-run the script "
        "after editing skill trees._",
        "",
    ]
    seen = set()
    order = TREE_ORDER + [(k, k) for k in by_tree if k not in dict(TREE_ORDER)]
    for key, heading in order:
        items = by_tree.get(key)
        if not items or key in seen:
            continue
        seen.add(key)
        lines.append(f"## {heading}")
        lines.append("")
        for n in sorted(items, key=lambda x: x["id"]):
            prereqs = n.get("prereqs", [])
            pretty_pre = ", ".join(by_id.get(p, str(p)) for p in prereqs) if prereqs else "—"
            eff = "; ".join(n.get("effects", [])) or "—"
            lines.append(
                f"- **{n.get('name','?')}** · T{n.get('tier','?')} · "
                f"{n.get('cost','?')} SP · prereq: {pretty_pre} · {eff}"
            )
            if n.get("description"):
                lines.append(f"  {n['description']}")
        lines.append("")

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))
    print(f"Wrote {OUT} ({len(all_nodes)} nodes, {len(by_tree)} trees)")


if __name__ == "__main__":
    main()
