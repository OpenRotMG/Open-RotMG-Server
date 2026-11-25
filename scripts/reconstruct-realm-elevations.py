#!/usr/bin/env python3
import collections
import hashlib
import json
from pathlib import Path
import struct
import zlib

DATA = Path(__file__).resolve().parents[1] / "data"
METHOD = ("terrain-band-median-v1: every cell gets the lower median of the recovered FSoD heights "
          "of its terrain byte; authored reconstruction, not original per-cell height")


def terrain_cells(path):
    data = zlib.decompress(path.read_bytes())
    offset = 0

    def number(fmt):
        nonlocal offset
        value = struct.unpack_from(fmt, data, offset)[0]
        offset += struct.calcsize(fmt)
        return value

    def skip_text():
        nonlocal offset
        length = 0
        for shift in range(0, 35, 7):
            byte = number("<B")
            length |= (byte & 127) << shift
            if byte < 128:
                break
        offset += length

    terrain = []
    for _ in range(number("<h")):
        number("<h")
        skip_text()
        skip_text()
        terrain.append(number("<B"))
        number("<B")
    width, height = number("<i"), number("<i")
    indices = memoryview(data)[offset:].cast("H")
    if len(indices) != width * height:
        raise ValueError(f"{path}: truncated or trailing cells")
    return bytes(terrain[i] for i in indices), width, height


def lower_median(counts):
    total, seen = sum(counts.values()), 0
    for height in sorted(counts):
        seen += counts[height]
        if 2 * seen >= total:
            return height
    raise ValueError("terrain band absent from recovered calibration")


def main():
    manifest_path = DATA / "realm-elevations.json"
    manifest = json.loads(manifest_path.read_text())
    recovered = [layer for layer in manifest["layers"] if layer["provenance"]["kind"] == "recovered"]
    counts = collections.defaultdict(collections.Counter)
    for layer in recovered:
        terrain, _, _ = terrain_cells(DATA / layer["map"])
        heights = zlib.decompress((DATA / layer["file"]).read_bytes())
        for (band, height), n in collections.Counter(zip(terrain, heights)).items():
            counts[band][height] += n
    medians = [lower_median(counts[band]) for band in range(13)]
    table = bytes(medians) + bytes(256 - 13)
    layers = list(recovered)
    for recipe in sorted((DATA / "realms").glob("world[0-9][0-9].json")):
        output = Path("realms") / json.loads(recipe.read_text())["output"]
        raw = (DATA / output).read_bytes()
        terrain, width, height = terrain_cells(DATA / output)
        if max(terrain) > 12:
            raise ValueError(f"{output}: terrain byte outside the calibrated bands")
        heights = terrain.translate(table)
        compressed = zlib.compress(heights, 9)
        file = Path("realms") / recipe.stem / "elevation.zlib"
        (DATA / file).write_bytes(compressed)
        layers.append({
            "map": str(output), "map_sha256": hashlib.sha256(raw).hexdigest(),
            "dimensions": [width, height],
            "provenance": {"kind": "reconstructed", "method": METHOD,
                           "calibration": [layer["file"] for layer in recovered],
                           "terrain_heights": medians},
            "file": str(file), "sha256": hashlib.sha256(compressed).hexdigest(),
            "expanded_sha256": hashlib.sha256(heights).hexdigest(),
        })
        print(output, file, len(compressed))
    manifest["layers"] = layers
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print("terrain_heights", medians)


if __name__ == "__main__":
    main()
