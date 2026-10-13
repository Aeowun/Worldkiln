use super::types::{
    HeightfieldData, PLAINS_GENERATOR_IMPLEMENTATION_VERSION, PlainsSettings, TerrainData,
    TerrainGenerationError, TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
};
use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 11:32 -05:00
pub(super) fn generate(
    settings: &PlainsSettings,
    request: TerrainGenerationRequest,
) -> Result<TerrainResult, TerrainGenerationError> {
    let mut broad = FastNoiseLite::with_seed(settings.seed);
    broad.set_noise_type(Some(NoiseType::OpenSimplex2S));
    broad.set_frequency(Some(settings.noise_scale));
    broad.set_fractal_type(Some(FractalType::FBm));
    broad.set_fractal_octaves(Some(3));
    broad.set_fractal_lacunarity(Some(2.0));
    broad.set_fractal_gain(Some(0.5));

    let mut relief = FastNoiseLite::with_seed(settings.seed ^ 0x4c31_8e2d);
    relief.set_noise_type(Some(NoiseType::OpenSimplex2));
    relief.set_frequency(Some(settings.noise_scale * 3.7));

    let mut warp = FastNoiseLite::with_seed(settings.seed ^ 0x19a7_4b63);
    warp.set_domain_warp_type(Some(DomainWarpType::OpenSimplex2Reduced));
    warp.set_frequency(Some(settings.noise_scale * 0.5));
    warp.set_domain_warp_amp(Some(8.0));

    let width = request.size[0] as usize;
    let depth = request.size[2] as usize;
    let mut heights = Vec::with_capacity(width * depth);
    let min_height = request.position[1] as f32;
    let max_height = request.inclusive_max()?[1] as f32;

    for local_z in 0..depth {
        let world_z = (request.position[2] + local_z as i32) as f32;
        for local_x in 0..width {
            let world_x = (request.position[0] + local_x as i32) as f32;
            let (warped_x, warped_z) = warp.domain_warp_2d(world_x, world_z);
            let broad_value = broad.get_noise_2d(warped_x, warped_z);
            let relief_value = relief.get_noise_2d(warped_x, warped_z);
            let landform = broad_value * 0.78 + relief_value * 0.22;
            let height = (settings.base_height + settings.height_variation * landform)
                .clamp(min_height, max_height);
            heights.push(height);
        }
    }

    Ok(TerrainResult {
        request,
        generator: TerrainGeneratorType::Plains,
        generator_implementation_version: PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
        data: TerrainData::Heightfield(HeightfieldData {
            origin: request.position,
            size: request.size.map(|value| value as u32),
            heights,
        }),
    })
}
