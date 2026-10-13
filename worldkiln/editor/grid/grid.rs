use crate::editor::GridPlane;
use crate::renderer::mesh::add_line;

pub const GRID_RADIUS: i32 = 8;

const GRID_PRIMARY_COLOR: [f32; 4] = [0.32, 0.34, 0.38, 0.52];
const GRID_MINOR_COLOR: [f32; 4] = [0.20, 0.22, 0.25, 0.28];
const GRID_BACK_COLOR: [f32; 4] = [0.14, 0.16, 0.19, 0.16];
const GRID_MAJOR_COLOR: [f32; 4] = [0.40, 0.42, 0.46, 0.68];
const GRID_BORDER_COLOR: [f32; 4] = [0.48, 0.50, 0.54, 0.78];

const X_AXIS_COLOR: [f32; 4] = [0.72, 0.28, 0.28, 0.90];
const Y_AXIS_COLOR: [f32; 4] = [0.30, 0.72, 0.34, 0.90];
const Z_AXIS_COLOR: [f32; 4] = [0.30, 0.48, 0.78, 0.90];

const MAJOR_GRID_SPACING: i32 = 4;

pub fn generate_grid_vertices(plane: GridPlane) -> Vec<f32> {
    let radius = GRID_RADIUS as f32;
    let line_count = (GRID_RADIUS * 2 + 1) as usize;

    // Two layers, two directions, one line per coordinate.
    let estimated_lines = line_count * 4;
    let mut vertices = Vec::with_capacity(estimated_lines * 42);

    // Main editing plane.
    add_grid_plane(&mut vertices, plane, 0.0, radius, false);

    // Backing layer gives the editor a little depth when working between
    // adjacent voxel layers without overpowering the active grid.
    add_grid_plane(&mut vertices, plane, -1.0, radius, true);

    // Strong world-axis guides on the active plane.
    add_axis_lines(&mut vertices, plane, radius);

    vertices
}

fn add_grid_plane(
    vertices: &mut Vec<f32>,
    plane: GridPlane,
    offset: f32,
    radius: f32,
    is_back_plane: bool,
) {
    for i in -GRID_RADIUS..=GRID_RADIUS {
        let position = i as f32;

        let color = if is_back_plane {
            GRID_BACK_COLOR
        } else if i.abs() == GRID_RADIUS {
            GRID_BORDER_COLOR
        } else if i % MAJOR_GRID_SPACING == 0 {
            GRID_MAJOR_COLOR
        } else {
            GRID_PRIMARY_COLOR
        };

        add_plane_lines(vertices, plane, offset, position, color, radius);
    }
}

fn add_plane_lines(
    vertices: &mut Vec<f32>,
    plane: GridPlane,
    offset: f32,
    position: f32,
    color: [f32; 4],
    radius: f32,
) {
    match plane {
        GridPlane::Xz => {
            add_line(
                vertices,
                [-radius, offset, position],
                [radius, offset, position],
                color,
            );

            add_line(
                vertices,
                [position, offset, -radius],
                [position, offset, radius],
                color,
            );
        }
        GridPlane::Yz => {
            add_line(
                vertices,
                [offset, -radius, position],
                [offset, radius, position],
                color,
            );

            add_line(
                vertices,
                [offset, position, -radius],
                [offset, position, radius],
                color,
            );
        }
        GridPlane::Xy => {
            add_line(
                vertices,
                [-radius, position, offset],
                [radius, position, offset],
                color,
            );

            add_line(
                vertices,
                [position, -radius, offset],
                [position, radius, offset],
                color,
            );
        }
    }
}

fn add_axis_lines(vertices: &mut Vec<f32>, plane: GridPlane, radius: f32) {
    match plane {
        GridPlane::Xz => {
            // World X axis.
            add_line(
                vertices,
                [-radius, 0.0, 0.0],
                [radius, 0.0, 0.0],
                X_AXIS_COLOR,
            );

            // World Z axis.
            add_line(
                vertices,
                [0.0, 0.0, -radius],
                [0.0, 0.0, radius],
                Z_AXIS_COLOR,
            );
        }
        GridPlane::Yz => {
            // World Y axis.
            add_line(
                vertices,
                [0.0, -radius, 0.0],
                [0.0, radius, 0.0],
                Y_AXIS_COLOR,
            );

            // World Z axis.
            add_line(
                vertices,
                [0.0, 0.0, -radius],
                [0.0, 0.0, radius],
                Z_AXIS_COLOR,
            );
        }
        GridPlane::Xy => {
            // World X axis.
            add_line(
                vertices,
                [-radius, 0.0, 0.0],
                [radius, 0.0, 0.0],
                X_AXIS_COLOR,
            );

            // World Y axis.
            add_line(
                vertices,
                [0.0, -radius, 0.0],
                [0.0, radius, 0.0],
                Y_AXIS_COLOR,
            );
        }
    }
}

pub fn select_grid_plane(camera_pos: glam::Vec3, target: glam::Vec3) -> GridPlane {
    let direction = camera_pos - target;

    // Avoid normalizing a zero-length vector.
    if direction.length_squared() <= f32::EPSILON {
        return GridPlane::Xz;
    }

    let direction = direction.normalize();
    let x = direction.x.abs();
    let y = direction.y.abs();
    let z = direction.z.abs();

    if y >= x && y >= z {
        GridPlane::Xz
    } else if x >= z {
        GridPlane::Yz
    } else {
        GridPlane::Xy
    }
}
