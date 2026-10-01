#!/usr/bin/env python3
"""Decode PNG (8-bit RGB/RGBA, filters 0-4) and report row brightness stats.
Usage: pngrow.py img.png [img.png ...]
Prints: size, and for each of bands (top/mid/bottom): mean brightness.
Flags shots where the bottom 12% is exactly-black while mid is not (paint artifact).
"""
import sys, struct, zlib

def read_png(path):
    d = open(path, 'rb').read()
    assert d[:8] == b'\x89PNG\r\n\x1a\n', path
    pos, idat, w = 8, b'', None
    ct = None
    while pos < len(d):
        ln = struct.unpack('>I', d[pos:pos+4])[0]
        typ = d[pos+4:pos+8]
        data = d[pos+8:pos+8+ln]
        if typ == b'IHDR':
            w, h, bit, color = struct.unpack('>IIBB', data[:10])
            assert bit == 8, f"{path}: bit depth {bit}"
            ct = color
        elif typ == b'IDAT':
            idat += data
        elif typ == b'IEND':
            break
        pos += 12 + ln
    raw = zlib.decompress(idat)
    nch = {0:1, 2:3, 4:2, 6:4}[ct]
    stride = w * nch
    rows = []
    prev = bytearray(stride)
    off = 0
    for y in range(h):
        f = raw[off]; off += 1
        line = bytearray(raw[off:off+stride]); off += stride
        if f == 1:
            for i in range(nch, stride): line[i] = (line[i] + line[i-nch]) & 255
        elif f == 2:
            for i in range(stride): line[i] = (line[i] + prev[i]) & 255
        elif f == 3:
            for i in range(stride):
                a = line[i-nch] if i >= nch else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 255
        elif f == 4:
            for i in range(stride):
                a = line[i-nch] if i >= nch else 0
                b = prev[i]
                c = prev[i-nch] if i >= nch else 0
                p = a + b - c
                pa, pb, pc = abs(p-a), abs(p-b), abs(p-c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        rows.append(bytes(line))
        prev = line
    return w, h, nch, rows

def rowmean(rows, y, nch, w):
    r = rows[y]
    s = 0
    n = 0
    for x in range(0, len(r), nch * 7):  # sample every 7th px
        px = r[x:x+nch]
        s += sum(px[:3]) / 3.0
        n += 1
    return s / max(n, 1)

def analyze(path):
    w, h, nch, rows = read_png(path)
    def band(a, b):
        return sum(rowmean(rows, y, nch, w) for y in range(a, b)) / max(b - a, 1)
    top = band(0, max(1, h // 10))
    mid = band(h // 3, h // 3 + max(1, h // 10))
    b0, b1 = int(h * 0.88), h
    bot = band(b0, b1)
    # exactly-black rows in bottom band (all sampled px == 0)
    exact_black = 0
    for y in range(b0, b1):
        r = rows[y]
        if not any(r[x] for x in range(0, len(r), nch)):
            exact_black += 1
    frac_black = exact_black / (b1 - b0)
    artifact = frac_black > 0.8 and mid > 5
    return dict(path=path, w=w, h=h, top=round(top,1), mid=round(mid,1),
                bottom=round(bot,1), frac_exact_black_bottom=round(frac_black,2),
                artifact=bool(artifact))

if __name__ == '__main__':
    for p in sys.argv[1:]:
        try:
            print(analyze(p))
        except Exception as e:
            print(dict(path=p, error=str(e)))
