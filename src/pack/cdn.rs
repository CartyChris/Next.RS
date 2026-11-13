//! Package CDN URLs → npm packages. A pinned file on unpkg or jsDelivr is the same file in
//! the registry tarball, and ESM CDNs (esm.sh, Skypack, jspm) serve converted builds of
//! packages the bundler can resolve itself. Anything else is fetched over HTTP.

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdnRef {
    /// One file inside a package version: `unpkg.com/three@0.160.0/build/three.module.js`.
    File { name: String, range: String, path: String },
    /// A package or a subpath of it, resolved through its `package.json`:
    /// `esm.sh/react@18`, `unpkg.com/lodash`, `cdn.jsdelivr.net/npm/three@0.160/+esm`.
    /// `esm` marks CDNs that convert to ES modules (vs. serving the package's default file).
    Package { name: String, range: String, subpath: Option<String>, esm: bool },
}

impl CdnRef {
    pub fn name(&self) -> &str {
        match self {
            CdnRef::File { name, .. } | CdnRef::Package { name, .. } => name,
        }
    }
    pub fn range(&self) -> &str {
        match self {
            CdnRef::File { range, .. } | CdnRef::Package { range, .. } => range,
        }
    }
}

/// `name[@range]` followed by the rest of the path. Scoped names take two segments.
fn split_package(path: &str) -> Option<(String, String, String)> {
    let path = path.trim_start_matches('/');
    let mut parts = path.splitn(if path.starts_with('@') { 3 } else { 2 }, '/');
    let (first, second) = (parts.next()?, if path.starts_with('@') { parts.next() } else { None });
    let rest = parts.next().unwrap_or("").to_string();
    let spec = match second {
        Some(s) => format!("{first}/{s}"),
        None => first.to_string(),
    };
    // The version starts at the last `@` that is not the scope's.
    let at = spec.char_indices().skip(1).filter(|(_, c)| *c == '@').map(|(i, _)| i).last();
    let (name, range) = match at {
        Some(i) => (spec[..i].to_string(), spec[i + 1..].to_string()),
        None => (spec, String::new()),
    };
    let range = percent_encoding::percent_decode_str(&range).decode_utf8_lossy().into_owned();
    crate::pack::npm::validate_name(&name).ok()?;
    Some((name, if range.is_empty() { "latest".into() } else { range }, rest))
}

pub fn parse(url: &Url) -> Option<CdnRef> {
    let host = url.host_str()?.trim_start_matches("www.");
    let path = url.path();
    let file = |name, range, rest: String| {
        if rest.is_empty() || rest.ends_with('/') {
            CdnRef::Package { name, range, subpath: None, esm: false }
        } else {
            CdnRef::File { name, range, path: rest }
        }
    };
    match host {
        "unpkg.com" => {
            let (name, range, rest) = split_package(path)?;
            Some(match (rest.is_empty(), url.query().is_some_and(|q| q.split('&').any(|p| p == "module"))) {
                (true, esm) => CdnRef::Package { name, range, subpath: None, esm },
                _ => file(name, range, rest),
            })
        }
        "cdn.jsdelivr.net" | "fastly.jsdelivr.net" | "gcore.jsdelivr.net" => {
            let (name, range, rest) = split_package(path.strip_prefix("/npm/")?)?;
            if rest == "+esm" {
                return Some(CdnRef::Package { name, range, subpath: None, esm: true });
            }
            if let Some(sub) = rest.strip_suffix("/+esm") {
                return Some(CdnRef::Package { name, range, subpath: Some(sub.to_string()), esm: true });
            }
            Some(file(name, range, rest))
        }
        "esm.run" => {
            let (name, range, rest) = split_package(path)?;
            Some(CdnRef::Package { name, range, subpath: (!rest.is_empty()).then_some(rest), esm: true })
        }
        "esm.sh" | "esm.sh.cn" => {
            // `/v135/`, `/stable/` prefixes; `*` marks external dependencies.
            let mut p = path.trim_start_matches('/');
            if let Some((first, rest)) = p.split_once('/') {
                if first == "stable" || (first.starts_with('v') && first[1..].chars().all(|c| c.is_ascii_digit())) {
                    p = rest;
                }
            }
            if p.starts_with("gh/") || p.starts_with("pr/") || p.starts_with("jsr/") {
                return None;
            }
            let (name, range, rest) = split_package(p.trim_start_matches('*'))?;
            let subpath = esm_sh_subpath(&name, &rest);
            Some(CdnRef::Package { name, range, subpath, esm: true })
        }
        "cdn.skypack.dev" => {
            // Pinned internal URLs: `/-/react@v17.0.1-yH0aYV1FOvoIPeKBbHxg/dist=es2019,mode=imports/optimized/react.js`.
            if let Some(internal) = path.strip_prefix("/-/") {
                let spec = internal.split("/dist=").next()?;
                let (name, range, _) = split_package(spec)?;
                let range = range.trim_start_matches('v').split('-').next().unwrap_or("").to_string();
                return Some(CdnRef::Package { name, range, subpath: None, esm: true });
            }
            let (name, range, rest) = split_package(path)?;
            Some(CdnRef::Package { name, range, subpath: (!rest.is_empty()).then_some(rest), esm: true })
        }
        "ga.jspm.io" | "jspm.dev" => {
            let p = path.trim_start_matches('/');
            match p.strip_prefix("npm:") {
                Some(spec) => {
                    let (name, range, rest) = split_package(spec)?;
                    Some(file(name, range, rest))
                }
                None => {
                    let (name, range, rest) = split_package(p)?;
                    Some(CdnRef::Package { name, range, subpath: (!rest.is_empty()).then_some(rest), esm: true })
                }
            }
        }
        _ => None,
    }
}

/// esm.sh build paths (`es2022/react.mjs`, `X-ZHJl.../es2022/jsx-runtime.mjs`) name a
/// package subpath by file; plain paths are subpaths as written.
fn esm_sh_subpath(name: &str, rest: &str) -> Option<String> {
    if rest.is_empty() {
        return None;
    }
    let parts: Vec<&str> = rest.split('/').collect();
    let target =
        parts.iter().position(|p| p.starts_with("es20") || matches!(*p, "esnext" | "denonext" | "deno" | "node"));
    match target {
        Some(i) => {
            let file = parts[i + 1..].join("/");
            let stem = file
                .trim_end_matches(".mjs")
                .trim_end_matches(".js")
                .trim_end_matches(".development")
                .trim_end_matches(".bundle");
            // `react.mjs` in package `react` (or `fiber.mjs` in `@react-three/fiber`) is the package itself.
            let own = name.rsplit('/').next().unwrap_or(name);
            (!stem.is_empty() && stem != own).then(|| stem.to_string())
        }
        None => Some(rest.to_string()),
    }
}

/// The `name/subpath` specifier a module import of `r` resolves like.
pub fn specifier(r: &CdnRef) -> String {
    match r {
        CdnRef::File { name, path, .. } => format!("{name}/{path}"),
        CdnRef::Package { name, subpath: Some(s), .. } => format!("{name}/{}", s.trim_end_matches('/')),
        CdnRef::Package { name, subpath: None, .. } => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(u: &str) -> Option<CdnRef> {
        parse(&Url::parse(u).unwrap())
    }
    fn file(name: &str, range: &str, path: &str) -> Option<CdnRef> {
        Some(CdnRef::File { name: name.into(), range: range.into(), path: path.into() })
    }
    fn pkg(name: &str, range: &str, subpath: Option<&str>, esm: bool) -> Option<CdnRef> {
        Some(CdnRef::Package { name: name.into(), range: range.into(), subpath: subpath.map(Into::into), esm })
    }

    #[test]
    fn cdn_urls() {
        assert_eq!(
            p("https://unpkg.com/three@0.160.0/build/three.module.js"),
            file("three", "0.160.0", "build/three.module.js")
        );
        assert_eq!(
            p("https://unpkg.com/react@18/umd/react.production.min.js"),
            file("react", "18", "umd/react.production.min.js")
        );
        assert_eq!(p("https://unpkg.com/lodash"), pkg("lodash", "latest", None, false));
        assert_eq!(p("https://unpkg.com/react@18?module"), pkg("react", "18", None, true));
        assert_eq!(
            p("https://unpkg.com/@react-three/fiber@8.15.0/dist/index.js"),
            file("@react-three/fiber", "8.15.0", "dist/index.js")
        );
        assert_eq!(p("https://unpkg.com/leaflet@%5E1.9/dist/leaflet.css"), file("leaflet", "^1.9", "dist/leaflet.css"));
        assert_eq!(
            p("https://cdn.jsdelivr.net/npm/three@0.160/examples/jsm/controls/OrbitControls.js"),
            file("three", "0.160", "examples/jsm/controls/OrbitControls.js")
        );
        assert_eq!(p("https://cdn.jsdelivr.net/npm/chart.js"), pkg("chart.js", "latest", None, false));
        assert_eq!(p("https://cdn.jsdelivr.net/npm/d3@7/+esm"), pkg("d3", "7", None, true));
        assert_eq!(p("https://cdn.jsdelivr.net/npm/@scope/x@1/sub/+esm"), pkg("@scope/x", "1", Some("sub"), true));
        assert_eq!(p("https://cdn.jsdelivr.net/gh/user/repo@1/x.js"), None);
        assert_eq!(p("https://esm.sh/react@18.2.0"), pkg("react", "18.2.0", None, true));
        assert_eq!(p("https://esm.sh/react-dom@18/client"), pkg("react-dom", "18", Some("client"), true));
        assert_eq!(p("https://esm.sh/v135/react@18.2.0/es2022/react.mjs"), pkg("react", "18.2.0", None, true));
        assert_eq!(
            p("https://esm.sh/stable/react@18.2.0/es2022/jsx-runtime.js"),
            pkg("react", "18.2.0", Some("jsx-runtime"), true)
        );
        assert_eq!(p("https://esm.sh/*preact@10?bundle"), pkg("preact", "10", None, true));
        assert_eq!(p("https://esm.sh/gh/user/repo"), None);
        assert_eq!(p("https://cdn.skypack.dev/canvas-confetti"), pkg("canvas-confetti", "latest", None, true));
        assert_eq!(
            p(
                "https://cdn.skypack.dev/-/react@v17.0.1-yH0aYV1FOvoIPeKBbHxg/dist=es2019,mode=imports/optimized/react.js"
            ),
            pkg("react", "17.0.1", None, true)
        );
        assert_eq!(p("https://ga.jspm.io/npm:lit@3.1.0/index.js"), file("lit", "3.1.0", "index.js"));
        assert_eq!(p("https://esm.run/canvas-confetti@1"), pkg("canvas-confetti", "1", None, true));
        assert_eq!(p("https://cdnjs.cloudflare.com/ajax/libs/three.js/r128/three.min.js"), None);
        assert_eq!(p("https://example.com/three@1/x.js"), None);
        assert_eq!(p("https://unpkg.com/.bin/x"), None);
    }

    #[test]
    fn specifiers() {
        assert_eq!(specifier(&pkg("react-dom", "18", Some("client"), true).unwrap()), "react-dom/client");
        assert_eq!(specifier(&file("three", "1", "build/three.module.js").unwrap()), "three/build/three.module.js");
    }
}
