pub mod load;
pub mod save;

#[cfg(test)]
mod tests;

pub use load::load_world;
pub use save::save_world;

pub const WORLD_FORMAT_VERSION: u32 = 2;
