#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct WorldCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl WorldCoord {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn from_vec3(pos: glam::Vec3) -> Self {
        Self::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        )
    }
}

impl From<(i32, i32, i32)> for WorldCoord {
    fn from(tuple: (i32, i32, i32)) -> Self {
        Self::new(tuple.0, tuple.1, tuple.2)
    }
}

impl From<WorldCoord> for (i32, i32, i32) {
    fn from(coord: WorldCoord) -> Self {
        (coord.x, coord.y, coord.z)
    }
}
