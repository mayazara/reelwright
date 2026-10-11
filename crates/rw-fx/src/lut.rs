//! 3D colour look-up tables in the `.cube` format.

/// A cubic RGB table. Inputs and outputs are display-referred (sRGB
/// encoded) values in `0..=1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    pub size: usize,
    /// `size³` entries, red varying fastest, then green, then blue.
    pub data: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CubeError {
    #[error("line {0}: {1}")]
    Syntax(usize, String),
    #[error("missing LUT_3D_SIZE")]
    NoSize,
    #[error("expected {expected} entries, found {found}")]
    Count { expected: usize, found: usize },
}

impl Lut {
    pub fn identity(size: usize) -> Lut {
        Lut::from_fn(size, |c| c)
    }

    /// Samples `f` on a `size`³ grid.
    pub fn from_fn(size: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> Lut {
        let step = 1.0 / (size - 1) as f32;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let out = f([r as f32 * step, g as f32 * step, b as f32 * step]);
                    data.push(out.map(|v| v.clamp(0.0, 1.0)));
                }
            }
        }
        Lut { size, data }
    }

    /// Parses `.cube` text (3D tables only; `DOMAIN_MIN`/`MAX` must be 0/1).
    pub fn parse(text: &str) -> Result<Lut, CubeError> {
        let mut size = None;
        let mut data = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let first = parts.next().unwrap_or_default();
            match first {
                "TITLE" | "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {}
                "LUT_1D_SIZE" => {
                    return Err(CubeError::Syntax(
                        n + 1,
                        "1D tables aren't supported".into(),
                    ));
                }
                "LUT_3D_SIZE" => {
                    let s: usize = parts
                        .next()
                        .and_then(|v| v.parse().ok())
                        .filter(|s| (2..=256).contains(s))
                        .ok_or_else(|| CubeError::Syntax(n + 1, "bad LUT_3D_SIZE".into()))?;
                    size = Some(s);
                    data.reserve(s * s * s);
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let want = if first == "DOMAIN_MIN" { 0.0 } else { 1.0 };
                    let ok = parts.all(|v| v.parse::<f32>().is_ok_and(|x| (x - want).abs() < 1e-6));
                    if !ok {
                        return Err(CubeError::Syntax(
                            n + 1,
                            "only a 0–1 domain is supported".into(),
                        ));
                    }
                }
                _ => {
                    let nums: Result<Vec<f32>, _> =
                        line.split_whitespace().map(str::parse::<f32>).collect();
                    match nums {
                        Ok(v) if v.len() == 3 => data.push([v[0], v[1], v[2]]),
                        _ => return Err(CubeError::Syntax(n + 1, format!("unexpected `{line}`"))),
                    }
                }
            }
        }
        let size = size.ok_or(CubeError::NoSize)?;
        let expected = size * size * size;
        if data.len() != expected {
            return Err(CubeError::Count {
                expected,
                found: data.len(),
            });
        }
        Ok(Lut { size, data })
    }

    /// Writes `.cube` text.
    pub fn to_cube(&self, title: &str, comments: &[&str]) -> String {
        let mut s = String::new();
        for c in comments {
            s.push_str("# ");
            s.push_str(c);
            s.push('\n');
        }
        s.push_str(&format!("TITLE \"{title}\"\nLUT_3D_SIZE {}\n", self.size));
        for [r, g, b] in &self.data {
            s.push_str(&format!("{r:.5} {g:.5} {b:.5}\n"));
        }
        s
    }

    fn at(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        self.data[(b * self.size + g) * self.size + r]
    }

    /// Trilinear lookup, as the GPU does it.
    pub fn sample(&self, c: [f32; 3]) -> [f32; 3] {
        let n = (self.size - 1) as f32;
        let p = c.map(|v| v.clamp(0.0, 1.0) * n);
        let i = p.map(|v| (v.floor() as usize).min(self.size - 2));
        let f = [p[0] - i[0] as f32, p[1] - i[1] as f32, p[2] - i[2] as f32];
        let mut out = [0.0; 3];
        for (db, wb) in [(0, 1.0 - f[2]), (1, f[2])] {
            for (dg, wg) in [(0, 1.0 - f[1]), (1, f[1])] {
                for (dr, wr) in [(0, 1.0 - f[0]), (1, f[0])] {
                    let v = self.at(i[0] + dr, i[1] + dg, i[2] + db);
                    let w = wr * wg * wb;
                    for k in 0..3 {
                        out[k] += v[k] * w;
                    }
                }
            }
        }
        out
    }

    /// RGBA8 texels for a GPU 3D texture.
    pub fn to_rgba8(&self) -> Vec<u8> {
        self.data
            .iter()
            .flat_map(|c| {
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                [q(c[0]), q(c[1]), q(c[2]), 255]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_identity_sampling() {
        let lut = Lut::from_fn(5, |[r, g, b]| [1.0 - r, g * 0.5, b]);
        let back = Lut::parse(&lut.to_cube("t", &["hello"])).unwrap();
        assert_eq!(back.size, 5);
        for (a, b) in lut.data.iter().zip(&back.data) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-4);
            }
        }
        let id = Lut::identity(17);
        let s = id.sample([0.3, 0.71, 0.05]);
        assert!((s[0] - 0.3).abs() < 1e-5 && (s[1] - 0.71).abs() < 1e-5);
    }

    #[test]
    fn rejects_bad_files() {
        assert_eq!(Lut::parse("0 0 0\n"), Err(CubeError::NoSize));
        assert!(matches!(
            Lut::parse("LUT_3D_SIZE 2\n0 0 0\n"),
            Err(CubeError::Count { .. })
        ));
        assert!(Lut::parse("LUT_1D_SIZE 4\n").is_err());
        assert!(Lut::parse("LUT_3D_SIZE 2\nDOMAIN_MAX 2 2 2\n").is_err());
        assert!(Lut::parse("LUT_3D_SIZE 2\nfoo bar\n").is_err());
    }
}
