import sys

path = sys.argv[1]
p = 4
with open(path, "rb") as fh:
    data = fh.read()
while p < len(data):
    hdr = data[p:p+4]
    head = hdr[0]
    typ = head & 0x7F
    last = head >> 7
    ln = int.from_bytes(hdr[1:4], "big")
    print(f"p={p} raw={hdr.hex()} last={last} type={typ} len={ln}")
    if typ == 4:
        body = p + 4
        vl = int.from_bytes(data[body:body+4], "little")
        q = body + 4 + vl
        cnt = int.from_bytes(data[q:q+4], "little")
        q += 4
        for i in range(min(cnt, 6)):
            l = int.from_bytes(data[q:q+4], "little")
            q += 4
            s = data[q:q+l].decode("utf-8", "replace")
            q += l
            print("   tag:", s)
    if typ == 6:
        body = p + 4
        q = body + 4
        ml = int.from_bytes(data[q:q+4], "big"); q += 4
        mime = data[q:q+ml].decode("utf-8", "replace"); q += ml
        dl = int.from_bytes(data[q:q+4], "big"); q += 4
        desc = data[q:q+dl]; q += dl
        w = int.from_bytes(data[q:q+4], "big"); q += 4
        h = int.from_bytes(data[q:q+4], "big"); q += 4
        depth = int.from_bytes(data[q:q+4], "big"); q += 4
        colors = int.from_bytes(data[q:q+4], "big"); q += 4
        dlen = int.from_bytes(data[q:q+4], "big"); q += 4
        print(f"   pic: {mime} {w}x{h} depth={depth} colors={colors} bytes={dlen} desc={desc[:40]!r}")
    p += 4 + ln
    if last:
        print("(last block reached)")
        break
