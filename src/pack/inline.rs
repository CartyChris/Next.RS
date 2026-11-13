//! Compressed classic scripts and stylesheets for packed pages.
//!
//! A module script can inflate asynchronously (`DecompressionStream`), but classic scripts
//! and styles must take effect at their place in the document: a script after them may use
//! their globals, and a late stylesheet flashes unstyled content. So they are deflated,
//! stored as base-122 text, and a small synchronous inflater (RFC 1951) turns them back into
//! a `<script>` / `<style>` element inserted right after the loader, which the browser runs
//! or applies immediately.

use std::fmt::Write as _;

use crate::{build::html, config::Compress};

/// Defines `__forgeInflate(text, packedLength, length)` → string. Readable source below;
/// `INFLATE_JS` is the same code with short names.
#[cfg(test)]
const INFLATE_SRC: &str = r#"
globalThis.__forgeInflateBytes = (d, n) => {
  const o = new Uint8Array(n);
  let p = 0, bb = 0, bc = 0, op = 0;
  const bits = (k) => {
    while (bc < k) { bb |= d[p++] << bc; bc += 8; }
    const v = bb & ((1 << k) - 1);
    bb >>>= k; bc -= k;
    return v;
  };
  const build = (lens, num) => {
    const count = new Uint16Array(16), sym = new Uint16Array(num), off = new Uint16Array(16);
    for (let i = 0; i < num; i++) count[lens[i]]++;
    count[0] = 0;
    for (let i = 1; i < 16; i++) off[i] = off[i - 1] + count[i - 1];
    for (let i = 0; i < num; i++) if (lens[i]) sym[off[lens[i]]++] = i;
    return [count, sym];
  };
  const decode = (t) => {
    let code = 0, first = 0, index = 0;
    for (let len = 1; len < 16; len++) {
      code |= bits(1);
      const c = t[0][len];
      if (code - c < first) return t[1][index + code - first];
      index += c; first = (first + c) << 1; code <<= 1;
    }
    throw new Error("forge: corrupt data");
  };
  const lext = [], lbase = [], dext = [], dbase = [];
  for (let i = 0, b = 3; i < 29; i++) { lext[i] = i < 8 || i == 28 ? 0 : (i - 4) >> 2; lbase[i] = i == 28 ? 258 : b; b += 1 << lext[i]; }
  for (let i = 0, b = 1; i < 30; i++) { dext[i] = i < 4 ? 0 : (i - 2) >> 1; dbase[i] = b; b += 1 << dext[i]; }
  const order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
  let last;
  do {
    last = bits(1);
    const type = bits(2);
    if (type == 0) {
      bb = bc = 0;
      const len = d[p] | (d[p + 1] << 8);
      p += 4;
      o.set(d.subarray(p, p + len), op);
      p += len; op += len;
      continue;
    }
    let lt, dt;
    if (type == 1) {
      const l = new Uint8Array(288);
      l.fill(8, 0, 144); l.fill(9, 144, 256); l.fill(7, 256, 280); l.fill(8, 280);
      lt = build(l, 288); dt = build(new Uint8Array(30).fill(5), 30);
    } else {
      const hlit = bits(5) + 257, hdist = bits(5) + 1, hclen = bits(4) + 4;
      const cl = new Uint8Array(19), lens = new Uint8Array(hlit + hdist);
      for (let i = 0; i < hclen; i++) cl[order[i]] = bits(3);
      const ct = build(cl, 19);
      for (let i = 0; i < hlit + hdist; ) {
        const s = decode(ct);
        if (s < 16) lens[i++] = s;
        else {
          let rep = s == 16 ? 3 + bits(2) : s == 17 ? 3 + bits(3) : 11 + bits(7);
          const v = s == 16 ? lens[i - 1] : 0;
          while (rep--) lens[i++] = v;
        }
      }
      lt = build(lens, hlit); dt = build(lens.subarray(hlit), hdist);
    }
    for (;;) {
      let s = decode(lt);
      if (s < 256) o[op++] = s;
      else if (s == 256) break;
      else {
        s -= 257;
        const len = bits(lext[s]) + lbase[s], ds = decode(dt), dist = bits(dext[ds]) + dbase[ds];
        for (let i = 0; i < len; i++, op++) o[op] = o[op - dist];
      }
    }
  } while (!last);
  return o;
};
"#;

/// Minified `INFLATE_SRC` plus the base-122 decoder and the string wrapper.
pub const INFLATE_JS: &str = concat!(
    r#"globalThis.__forgeInflateBytes=(d,n)=>{const o=new Uint8Array(n);let p=0,B=0,C=0,q=0;const b=k=>{for(;C<k;)B|=d[p++]<<C,C+=8;const v=B&(1<<k)-1;return B>>>=k,C-=k,v},h=(l,m)=>{const c=new Uint16Array(16),s=new Uint16Array(m),f=new Uint16Array(16);for(let i=0;i<m;i++)c[l[i]]++;c[0]=0;for(let i=1;i<16;i++)f[i]=f[i-1]+c[i-1];for(let i=0;i<m;i++)l[i]&&(s[f[l[i]]++]=i);return[c,s]},x=t=>{let c=0,f=0,j=0;for(let n=1;n<16;n++){c|=b(1);const k=t[0][n];if(c-k<f)return t[1][j+c-f];j+=k,f=f+k<<1,c<<=1}throw Error("forge: corrupt data")},E=[],L=[],F=[],D=[];for(let i=0,s=3;i<29;i++)E[i]=i<8||i==28?0:i-4>>2,L[i]=i==28?258:s,s+=1<<E[i];for(let i=0,s=1;i<30;i++)F[i]=i<4?0:i-2>>1,D[i]=s,s+=1<<F[i];const O=[16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15];let z;do{z=b(1);const y=b(2);if(y==0){B=C=0;const n=d[p]|d[p+1]<<8;p+=4,o.set(d.subarray(p,p+n),q),p+=n,q+=n;continue}let a,g;if(y==1){const l=new Uint8Array(288);l.fill(8,0,144),l.fill(9,144,256),l.fill(7,256,280),l.fill(8,280),a=h(l,288),g=h(new Uint8Array(30).fill(5),30)}else{const H=b(5)+257,I=b(5)+1,K=b(4)+4,c=new Uint8Array(19),l=new Uint8Array(H+I);for(let i=0;i<K;i++)c[O[i]]=b(3);const t=h(c,19);for(let i=0;i<H+I;){const s=x(t);if(s<16)l[i++]=s;else{let r=s==16?3+b(2):s==17?3+b(3):11+b(7);const v=s==16?l[i-1]:0;for(;r--;)l[i++]=v}}a=h(l,H),g=h(l.subarray(H),I)}for(;;){let s=x(a);if(s<256)o[q++]=s;else{if(s==256)break;s-=257;const n=b(E[s])+L[s],e=x(g),t=b(F[e])+D[e];for(let i=0;i<n;i++,q++)o[q]=o[q-t]}}}while(!z);return o};"#,
    r#"globalThis.__forgeInflate=(s,n,m)=>{const I=[0,10,13,34,92,60],d=new Uint8Array(n);let a=0,k=0,j=0;const p=g=>{a=(a<<7|g)&16383;if((k+=7)>7)d[j++]=a>>(k-=8)};for(let i=0;i<s.length;i++){const c=s.charCodeAt(i);c>127?(p(I[(c>>7)-1]),p(c&127)):p(c)}return new TextDecoder().decode(__forgeInflateBytes(d,m))};"#
);

/// Compress `text` for inline use if it pays off: `(base-122 text, packed length, length)`.
pub fn pack(text: &str, compress: Compress) -> Option<(String, usize, usize)> {
    if compress == Compress::Off || (compress == Compress::Auto && text.len() < 16 * 1024) {
        return None;
    }
    let packed = html::deflate(text.as_bytes());
    let encoded = html::base122(&packed);
    // The loader costs ~100 bytes per element; demand a quarter saved, as for modules.
    let worth = compress == Compress::On || (encoded.len() + 100) * 4 <= text.len() * 3;
    worth.then_some((encoded, packed.len(), text.len()))
}

/// A loader that inserts `<tag attrs>text</tag>` in its own place, synchronously.
pub fn loader(tag: &str, attrs: &[(String, String)], (encoded, packed, len): &(String, usize, usize)) -> String {
    let mut set = String::new();
    for (k, v) in attrs {
        let _ =
            write!(set, "e.setAttribute({},{});", serde_json::to_string(k).unwrap(), serde_json::to_string(v).unwrap());
    }
    let prop = if tag == "script" { "text" } else { "textContent" };
    format!(
        r#"<script>(()=>{{const e=document.createElement("{tag}");{set}e.{prop}=__forgeInflate("{encoded}",{packed},{len});document.currentScript.after(e)}})()</script>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode with both the readable and the minified inflater in QuickJS.
    fn inflate_in_js(packed: &[u8], len: usize) -> [Vec<u8>; 2] {
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            let run = |src: &str| -> Vec<u8> {
                ctx.eval::<(), _>(src).unwrap();
                let input = rquickjs::TypedArray::<u8>::new(ctx.clone(), packed.to_vec()).unwrap();
                let f: rquickjs::Function = ctx.eval("(d, n) => Array.from(__forgeInflateBytes(d, n))").unwrap();
                f.call((input, len)).unwrap()
            };
            [run(INFLATE_SRC), run(INFLATE_JS)]
        })
    }

    #[test]
    fn js_inflate_matches_deflate() {
        let mut x = 0x1234_5678_9abc_def0u64;
        let mut rand = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let text: String = (0..30_000).map(|i| format!("let v{i} = {};\n", rand() % 1000)).collect();
        let noise: Vec<u8> = (0..70_000).map(|_| rand() as u8).collect();
        let cases: Vec<(Vec<u8>, &str)> = vec![
            (Vec::new(), "empty"),
            (b"a".to_vec(), "one byte (fixed Huffman)"),
            (text.into_bytes(), "text (dynamic Huffman, long matches)"),
            (noise, "noise (stored blocks)"),
        ];
        for (data, what) in cases {
            for level in [1, 9] {
                let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
                std::io::Write::write_all(&mut enc, &data).unwrap();
                let packed = enc.finish().unwrap();
                for out in inflate_in_js(&packed, data.len()) {
                    assert!(out == data, "{what} at level {level}: {} bytes back, {} expected", out.len(), data.len());
                }
            }
        }
    }

    #[test]
    fn packing_needs_size_and_savings() {
        assert!(pack("x".repeat(100).as_str(), Compress::Auto).is_none());
        assert!(pack("x".repeat(100).as_str(), Compress::On).is_some());
        let big = "function f(a){return a+1}\n".repeat(2000);
        let (encoded, _, len) = pack(&big, Compress::Auto).unwrap();
        assert!(encoded.len() * 10 < len && !encoded.contains('<'));
        assert!(pack(&big, Compress::Off).is_none());
        let tag = loader("style", &[("media".into(), "print".into())], &(encoded, 1, 2));
        assert!(
            tag.starts_with("<script>")
                && tag.contains(r#"e.setAttribute("media","print")"#)
                && tag.contains("e.textContent=")
        );
    }
}
