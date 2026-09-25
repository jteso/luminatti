//! Optional, local TALA geometry worker. The native layout remains available
//! when the helper is absent, a graph is too large, or its time budget expires.
use super::layout::Position;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct Request<'a> {
    sizes: &'a [(f32, f32)],
    edges: &'a [(usize, usize)],
}

#[derive(Deserialize)]
pub(super) struct Geometry {
    pub positions: Vec<Position>,
    pub routes: Vec<Vec<Position>>,
}

pub(super) fn executable() -> Option<PathBuf> {
    let path = if let Some(path) = std::env::var_os("LUMINATTI_RADAR_LAYOUT") {
        PathBuf::from(path)
    } else {
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("luminatti-radar-layout")
    };
    (path.is_absolute() && path.is_file()).then_some(path)
}

pub(super) fn layout(sizes: &[(f32, f32)], edges: &[(usize, usize)]) -> Option<Geometry> {
    if sizes.is_empty() || sizes.len() > 160 || edges.len() > 600 {
        return None;
    }
    let input = serde_json::to_vec(&Request { sizes, edges }).ok()?;
    let mut child = Command::new(executable()?)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Drain stdout while the child runs so a large route result cannot fill
    // the pipe and deadlock. Bound both output and wall-clock runtime.
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout.take(2 * 1024 * 1024).read_to_end(&mut output).ok()?;
        Some(output)
    });
    let mut stdin = child.stdin.take()?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let deadline = Instant::now() + Duration::from_secs(5);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let written = writer.join().ok()?.is_ok();
    let output = reader.join().ok()??;
    if !success || !written {
        return None;
    }
    let geometry: Geometry = serde_json::from_slice(&output).ok()?;
    geometry.valid(sizes, edges).then_some(geometry)
}

impl Geometry {
    fn valid(&self, sizes: &[(f32, f32)], edges: &[(usize, usize)]) -> bool {
        if self.positions.len() != sizes.len() || self.routes.len() != edges.len() {
            return false;
        }
        let finite = |p: &Position| {
            p.x.is_finite() && p.y.is_finite() && p.x.abs().max(p.y.abs()) < 1_000_000.
        };
        if !self.positions.iter().all(finite) {
            return false;
        }
        // Reject overlaps or broken routing rather than replacing a usable
        // diagram with partial/incompatible helper output.
        for (i, a) in self.positions.iter().enumerate() {
            for (j, b) in self.positions.iter().enumerate().skip(i + 1) {
                if a.x < b.x + sizes[j].0
                    && a.x + sizes[i].0 > b.x
                    && a.y < b.y + sizes[j].1
                    && a.y + sizes[i].1 > b.y
                {
                    return false;
                }
            }
        }
        for (route, &(from, to)) in self.routes.iter().zip(edges) {
            if route.len() < 2 || !route.iter().all(finite) {
                return false;
            }
            let on_boundary = |p: Position, i: usize| {
                let origin = self.positions[i];
                let x = p.x - origin.x;
                let y = p.y - origin.y;
                x >= -2.
                    && y >= -2.
                    && x <= sizes[i].0 + 2.
                    && y <= sizes[i].1 + 2.
                    && (x.abs() <= 2.
                        || y.abs() <= 2.
                        || (x - sizes[i].0).abs() <= 2.
                        || (y - sizes[i].1).abs() <= 2.)
            };
            if !on_boundary(route[0], from) || !on_boundary(*route.last().unwrap(), to) {
                return false;
            }
            for pair in route.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if (a.x - b.x).abs() > 0.1 && (a.y - b.y).abs() > 0.1 {
                    return false;
                }
                for (i, p) in self.positions.iter().enumerate() {
                    let crosses = if (a.x - b.x).abs() <= 0.1 {
                        a.x > p.x + 1.
                            && a.x < p.x + sizes[i].0 - 1.
                            && a.y.min(b.y) < p.y + sizes[i].1 - 1.
                            && a.y.max(b.y) > p.y + 1.
                    } else {
                        a.y > p.y + 1.
                            && a.y < p.y + sizes[i].1 - 1.
                            && a.x.min(b.x) < p.x + sizes[i].0 - 1.
                            && a.x.max(b.x) > p.x + 1.
                    };
                    if crosses {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incomplete_overlapping_and_obstructed_geometry() {
        let p = |x, y| Position { x, y };
        let sizes = [(100., 60.); 3];
        let edges = [(0, 2)];
        let mut geometry = Geometry {
            positions: vec![p(0., 0.), p(150., 0.), p(300., 0.)],
            routes: vec![vec![
                p(100., 30.),
                p(120., 30.),
                p(120., 90.),
                p(280., 90.),
                p(280., 30.),
                p(300., 30.),
            ]],
        };
        assert!(geometry.valid(&sizes, &edges));
        geometry.routes[0] = vec![p(100., 30.), p(300., 30.)];
        assert!(
            !geometry.valid(&sizes, &edges),
            "route crosses the middle card"
        );
        geometry.positions[1] = p(50., 0.);
        assert!(!geometry.valid(&sizes, &edges), "cards overlap");
        geometry.positions.pop();
        assert!(!geometry.valid(&sizes, &edges), "missing card");
    }

    #[test]
    fn rejects_diagonal_detached_and_nonfinite_routes() {
        let sizes = [(100., 60.); 2];
        let edges = [(0, 1)];
        let mut geometry = Geometry {
            positions: vec![Position { x: 0., y: 0. }, Position { x: 150., y: 100. }],
            routes: vec![vec![
                Position { x: 100., y: 30. },
                Position { x: 150., y: 130. },
            ]],
        };
        assert!(!geometry.valid(&sizes, &edges));
        geometry.routes[0] = vec![Position { x: 110., y: 130. }, Position { x: 150., y: 130. }];
        assert!(!geometry.valid(&sizes, &edges));
        geometry.positions[0].x = f32::NAN;
        assert!(!geometry.valid(&sizes, &edges));
    }
}
