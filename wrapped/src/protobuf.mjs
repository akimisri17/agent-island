// Just enough of the protobuf wire format to read a few known fields from
// blobs whose schema is not published. Field paths are numbers, e.g. [4, 2].
// Malformed input throws; callers treat that as "field not present".

function varint(buf, i) {
  let value = 0;
  let scale = 1;
  for (;;) {
    if (i >= buf.length) throw new Error('truncated varint');
    const b = buf[i++];
    value += (b & 0x7f) * scale; // multiply, not shift: values can exceed 32 bits
    if (b < 0x80) return [value, i];
    scale *= 128;
  }
}

// Yields { field, wire, value }: value is a number for varints, bytes otherwise.
export function* fields(buf) {
  let i = 0;
  while (i < buf.length) {
    const [key, j] = varint(buf, i);
    const field = Math.floor(key / 8);
    const wire = key % 8;
    i = j;
    if (wire === 0) {
      const [v, k] = varint(buf, i);
      i = k;
      yield { field, wire, value: v };
    } else if (wire === 2) {
      const [len, k] = varint(buf, i);
      if (k + len > buf.length) throw new Error('truncated field');
      yield { field, wire, value: buf.subarray(k, k + len) };
      i = k + len;
    } else if (wire === 1 || wire === 5) {
      const n = wire === 1 ? 8 : 4;
      yield { field, wire, value: buf.subarray(i, i + n) };
      i += n;
    } else {
      throw new Error(`unsupported wire type ${wire}`);
    }
  }
}

// First value at a path of field numbers, or undefined.
export function get(buf, path) {
  try {
    let cur = buf;
    for (let d = 0; d < path.length; d++) {
      let next;
      for (const f of fields(cur)) {
        if (f.field === path[d]) {
          next = f;
          break;
        }
      }
      if (!next) return undefined;
      if (d === path.length - 1) return next.value;
      if (next.wire !== 2) return undefined;
      cur = next.value;
    }
  } catch {
    return undefined;
  }
  return undefined;
}

export function getString(buf, path) {
  const v = get(buf, path);
  return v instanceof Uint8Array ? Buffer.from(v).toString('utf8') : undefined;
}

// google.protobuf.Timestamp { 1: seconds, 2: nanos } at path, in ms.
export function getTimestamp(buf, path) {
  const msg = get(buf, path);
  if (!(msg instanceof Uint8Array)) return undefined;
  const seconds = get(msg, [1]);
  if (typeof seconds !== 'number') return undefined;
  const nanos = get(msg, [2]);
  return seconds * 1000 + Math.floor((typeof nanos === 'number' ? nanos : 0) / 1e6);
}
