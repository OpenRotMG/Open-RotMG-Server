use super::*;

const ELEVATION_BLOCK: i32 = 16;

pub(super) struct ElevationBlocks {
    columns: usize,
    maxima: Box<[u8]>,
}

impl ElevationBlocks {
    pub(super) fn new(map: &Map) -> Result<Option<Self>> {
        let Some(layer) = map.elevation.as_deref() else {
            return Ok(None);
        };
        let (width, height) = (map.width as usize, map.height as usize);
        ensure!(layer.len() == width * height, "map elevation size mismatch");
        let side = ELEVATION_BLOCK as usize;
        let columns = width.div_ceil(side);
        let mut maxima = vec![0u8; columns * height.div_ceil(side)];
        for (y, row) in layer.chunks_exact(width).enumerate() {
            for (x, span) in row.chunks(side).enumerate() {
                let max = &mut maxima[y / side * columns + x];
                *max = (*max).max(*span.iter().max().expect("nonempty elevation span"));
            }
        }
        Ok(Some(Self {
            columns,
            maxima: maxima.into_boxed_slice(),
        }))
    }

    fn max(&self, column: i32, row: i32) -> f64 {
        f64::from(self.maxima[row as usize * self.columns + column as usize])
    }
}

impl World {
    pub(super) fn xml_environment_goal(
        &self,
        b: &Behavior,
        pos: Pos,
        kind: u16,
        scope: Scope,
    ) -> Result<Option<Pos>> {
        ensure!(
            pos.x.is_finite() && pos.y.is_finite() && self.map.cell(pos.x, pos.y).is_some(),
            "{} invalid environment search origin {},{}",
            b.source,
            pos.x,
            pos.y
        );
        let (width, height) = (self.map.width, self.map.height);
        let cx = pos.x.floor() as i32;
        let cy = pos.y.floor() as i32;
        let altitude = if b.kind == K::StayAbove {
            if !admission::stay_above_applies(self.xml.domain, b)? {
                return Ok(None);
            }
            let layer = self.map.elevation.as_deref().with_context(|| {
                format!(
                    "{} map={} missing source elevation capability",
                    b.source, self.name
                )
            })?;
            let minimum = n(b, F::MinAltitude);
            if f64::from(layer[(cy * width + cx) as usize]) >= minimum {
                return Ok(None);
            }
            let blocks = self
                .xml
                .elevation_blocks
                .as_ref()
                .context("map elevation index absent")?;
            Some((layer, minimum, blocks))
        } else {
            None
        };
        let matches = |p: Pos| -> Result<bool> {
            if b.kind == K::Walk {
                let c = self.map.cell(p.x, p.y).context("Walk cell absent")?;
                let Some(Value::Grounds(gs)) = value(b, F::GroundTypes) else {
                    bail!("Walk ground types absent");
                };
                Ok(gs.contains(&c.ground))
            } else {
                self.xml_terrain_matches(b, p, kind)
            }
        };
        if altitude.is_none() && b.kind != K::Walk && matches(pos)? {
            return Ok(None);
        }
        // Whole reconstructed altitude bands can be ineligible. Skip their blocks, but keep
        // exact tile distances and current collision checks in every qualifying block.
        let side = if altitude.is_some() {
            ELEVATION_BLOCK
        } else {
            1
        };
        let columns = (width - 1) / side + 1;
        let rows = (height - 1) / side + 1;
        let (cx, cy) = (cx / side, cy / side);
        let limit = cx.max(cy).max(columns - 1 - cx).max(rows - 1 - cy);
        let mut best: Option<(f64, i32, i32)> = None;
        let visit = |column: i32, row: i32, best: &mut Option<(f64, i32, i32)>| -> Result<()> {
            if altitude.is_some_and(|(_, minimum, blocks)| blocks.max(column, row) < minimum) {
                return Ok(());
            }
            for y in row * side..((row + 1) * side).min(height) {
                for x in column * side..((column + 1) * side).min(width) {
                    if altitude.is_some_and(|(layer, minimum, _)| {
                        f64::from(layer[(y * width + x) as usize]) < minimum
                    }) {
                        continue;
                    }
                    let p = Pos {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    let distance = (f64::from(p.x) - f64::from(pos.x)).powi(2)
                        + (f64::from(p.y) - f64::from(pos.y)).powi(2);
                    let candidate = (distance, y, x);
                    if distance >= 0.0001
                        && best.is_none_or(|current| candidate < current)
                        && (altitude.is_some() || matches(p)?)
                        && self.xml_npc_walkable(scope, p)?
                    {
                        *best = Some(candidate);
                    }
                }
            }
            Ok(())
        };
        for radius in 0..=limit {
            let (left, right, top, bottom) = (cx - radius, cx + radius, cy - radius, cy + radius);
            for y in [top, bottom] {
                if (0..rows).contains(&y) {
                    for x in left.max(0)..=right.min(columns - 1) {
                        visit(x, y, &mut best)?;
                    }
                }
                if top == bottom {
                    break;
                }
            }
            for x in [left, right] {
                if (0..columns).contains(&x) {
                    for y in (top + 1).max(0)..=(bottom - 1).min(rows - 1) {
                        visit(x, y, &mut best)?;
                    }
                }
                if left == right {
                    break;
                }
            }
            // Strict inequality preserves ties with tiles in a later block.
            let next = [
                (left > 0).then_some(f64::from(pos.x) - (f64::from(left * side) - 0.5)),
                (right < columns - 1)
                    .then_some(f64::from((right + 1) * side) + 0.5 - f64::from(pos.x)),
                (top > 0).then_some(f64::from(pos.y) - (f64::from(top * side) - 0.5)),
                (bottom < rows - 1)
                    .then_some(f64::from((bottom + 1) * side) + 0.5 - f64::from(pos.y)),
            ]
            .into_iter()
            .flatten()
            .reduce(f64::min);
            if next
                .is_none_or(|bound| best.is_some_and(|(distance, _, _)| distance < bound.powi(2)))
            {
                break;
            }
        }
        Ok(best.map(|(_, y, x)| Pos {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
        }))
    }

    pub(super) fn xml_terrain_matches(&self, b: &Behavior, pos: Pos, kind: u16) -> Result<bool> {
        let cell = self
            .map
            .cell(pos.x, pos.y)
            .context("movement destination outside map")?;
        let terrain = match value(b, F::Terrain) {
            Some(Value::Grounds(gs)) => gs.contains(&cell.ground),
            Some(Value::NativeTerrain) | None => {
                cell.terrain == self.content.objects[&kind].terrain
            }
            _ => bail!("invalid linked terrain"),
        };
        let altitude =
            if b.fields.contains_key(&F::MinAltitude) || b.fields.contains_key(&F::MaxAltitude) {
                let e = self.map.elevation_at(pos.x, pos.y).with_context(|| {
                    format!(
                        "{} missing elevation map={} cell={},{}",
                        b.source, self.name, pos.x, pos.y
                    )
                })?;
                // StayInTerrain declares either altitude bound alone; an absent bound does not constrain.
                opt_n(b, F::MinAltitude).is_none_or(|min| f64::from(e) >= min)
                    && opt_n(b, F::MaxAltitude).is_none_or(|max| f64::from(e) <= max)
            } else {
                true
            };
        Ok(terrain && altitude)
    }
}
