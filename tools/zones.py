"""Builds mod/data/npc_zones.tsv from the enemy placements of the game's maps.

Input: msb_enemies.tsv (map, NpcParamId, ThinkParamId, part, model), written by
tools/msbdump (reads every MSB of the packed game with Smithbox's libraries,
read only), and Smithbox's map names (Assets/Aliases/ER/MapNames.json, MIT).

Output (UTF-8, tab separated):
  Z <index> <group> <zone>          one line per zone
  <NpcParamId> <zone index>,... <ThinkParamId>
                                    the zones a row is placed in, and the AI
                                    the maps give it most often (0 = none)
"""
import collections, json, re, sys

enemies, map_names, out = sys.argv[1:4]
names = {x["ID"]: x["Name"] for x in json.load(open(map_names, encoding="utf-8"))}

GROUPS = {
    "60": "The Lands Between",
    "61": "Realm of Shadow",
    "30": "Catacombs",
    "31": "Caves",
    "32": "Tunnels",
    "34": "Divine towers",
    "35": "Legacy dungeons",
    "39": "Legacy dungeons",
    "40": "Realm of Shadow dungeons",
    "41": "Realm of Shadow dungeons",
    "42": "Realm of Shadow dungeons",
    "43": "Realm of Shadow dungeons",
    "45": "Colosseums",
}


# Sub-areas folded into the zone players know.
MERGE = {
    "Liurnia": "Liurnia of the Lakes",
    "Liurnia of the Lake": "Liurnia of the Lakes",
    "Liurnia to Altus Plateau": "Liurnia of the Lakes",
    "Bellum Highway": "Liurnia of the Lakes",
    "Snowfield": "Consecrated Snowfield",
    "Ordina": "Consecrated Snowfield",
    "Mountaintops": "Mountaintops of the Giants",
    "Flame Peak": "Mountaintops of the Giants",
    "Weeping Peninsula Coast": "Weeping Peninsula",
    "Leyndell": "Capital Outskirts",
    "Greyoll's Dragonbarrow": "Dragonbarrow",
    "Foot of the Jagged Peak": "Jagged Peak",
    "Lake of Rot": "Ainsel River / Lake of Rot",
    "Nokron": "Siofra River / Nokron",
    "Siofra River": "Siofra River / Nokron",
}


def clean(n):
    n = re.sub(r"\s*\((Post-Radahn|Unsealed|Meteor)[^)]*\)", "", n)
    n = n.split(";")[0].split(":")[0].strip()
    return n


def zone_of(m):
    a, b, c, d = m[1:3], m[4:6], m[7:9], m[10:12]
    if a in ("60", "61"):
        n = names.get(m) or ""
        if not n:
            for k in (f"m{a}_{int(b)//2:02d}_{int(c)//2:02d}_01", f"m{a}_{int(b)//4:02d}_{int(c)//4:02d}_02"):
                if names.get(k):
                    n = names[k]
                    break
        n = clean(n).split(" - ")[0].strip()
        # Overworld tile names start with a direction: "Southwest Liurnia".
        n = re.sub(r"^(Far )?(North|South|East|West|Central)[a-z-]*\s+", "", n)
        return GROUPS[a], MERGE.get(n, n) or "Open world"
    n = clean(names.get(m) or names.get(f"m{a}_{b}_{c}_00") or names.get(f"m{a}_{b}_00_00") or "")
    if not n:
        return None
    n = n.split(" - ")[0].strip()
    n = re.sub(r"\s*\([^)]*\)$", "", n)
    return GROUPS.get(a, "Legacy dungeons"), MERGE.get(n, n)


zones = {}
by_npc = collections.defaultdict(set)
thinks = collections.defaultdict(collections.Counter)
unknown = collections.Counter()
for line in open(enemies, encoding="utf-8"):
    m, npc, think = line.split("\t")[:3]
    npc = int(npc)
    if npc <= 0:
        continue
    if int(think) > 0:
        thinks[npc][int(think)] += 1
    z = zone_of(m)
    if z is None:
        unknown[m] += 1
        continue
    i = zones.setdefault(z, len(zones))
    by_npc[npc].add(i)

order = sorted(zones, key=lambda z: (list(dict.fromkeys(GROUPS.values())).index(z[0]) if z[0] in GROUPS.values() else 99, z[1]))
remap = {zones[z]: k for k, z in enumerate(order)}
with open(out, "w", encoding="utf-8", newline="\n") as f:
    for k, (g, n) in enumerate(order):
        f.write(f"Z\t{k}\t{g}\t{n}\n")
    for npc in sorted(by_npc):
        think = thinks[npc].most_common(1)[0][0] if thinks[npc] else 0
        f.write(f"{npc}\t{','.join(str(remap[i]) for i in sorted(by_npc[npc], key=lambda i: remap[i]))}\t{think}\n")
print(len(order), "zones,", len(by_npc), "NpcParam rows placed; unknown maps", dict(unknown))
for g in dict.fromkeys(z[0] for z in order):
    print(g, ":", ", ".join(z[1] for z in order if z[0] == g))
