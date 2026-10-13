//! Optional, asynchronous GPU timings for the standalone game renderer.

const QUERY_COUNT: usize = 6;
const SLOT_COUNT: usize = 4;
const REPORT_SAMPLES: u64 = 120;

#[derive(Clone, Copy, Default)]
pub(super) struct DrawStats {
    pub shadow_draws: u64,
    pub shadow_triangles: u64,
    pub static_draws: u64,
    pub static_triangles: u64,
    pub dynamic_draws: u64,
    pub dynamic_triangles: u64,
    pub terrain_draws: u64,
    pub terrain_triangles: u64,
    pub terrain_chunks_drawn: u64,
    pub terrain_chunks_culled: u64,
    pub water_draws: u64,
    pub water_triangles: u64,
}

#[derive(Default)]
struct QuerySlot {
    ids: [u32; QUERY_COUNT],
    pending: bool,
    stats: DrawStats,
}

pub(super) struct RenderProfiler {
    slots: [QuerySlot; SLOT_COUNT],
    active: Option<usize>,
    next_slot: usize,
    samples: u64,
    sums_ns: [u128; 5],
    sums_draws: DrawStats,
}

impl RenderProfiler {
    pub fn new() -> Option<Self> {
        if std::env::var("AEO_RENDER_PROFILE").as_deref() != Ok("1") {
            return None;
        }
        if !gl::QueryCounter::is_loaded()
            || !gl::GenQueries::is_loaded()
            || !gl::GetQueryObjectiv::is_loaded()
            || !gl::GetQueryObjectui64v::is_loaded()
        {
            eprintln!("AEO_RENDER_PROFILE: OpenGL timer queries unavailable");
            return None;
        }
        let mut profiler = Self {
            slots: std::array::from_fn(|_| QuerySlot::default()),
            active: None,
            next_slot: 0,
            samples: 0,
            sums_ns: [0; 5],
            sums_draws: DrawStats::default(),
        };
        unsafe {
            for slot in &mut profiler.slots {
                gl::GenQueries(QUERY_COUNT as i32, slot.ids.as_mut_ptr());
            }
        }
        eprintln!(
            "AEO_RENDER_PROFILE: sampling game renderer GPU time; reports every {REPORT_SAMPLES} completed frames"
        );
        Some(profiler)
    }

    pub fn begin(&mut self) {
        self.poll();
        let index = self.next_slot;
        self.next_slot = (self.next_slot + 1) % SLOT_COUNT;
        if self.slots[index].pending {
            self.active = None;
            return;
        }
        self.active = Some(index);
        unsafe { gl::QueryCounter(self.slots[index].ids[0], gl::TIMESTAMP) };
    }

    /// 1: shadow end; 2: static-cell start; 3: static-cell end / Terrain start;
    /// 4: opaque Terrain end; 5: frame end.
    pub fn mark(&self, boundary: usize) {
        if let Some(index) = self.active {
            unsafe { gl::QueryCounter(self.slots[index].ids[boundary], gl::TIMESTAMP) };
        }
    }

    pub fn end(&mut self, stats: DrawStats) {
        self.mark(5);
        if let Some(index) = self.active.take() {
            self.slots[index].stats = stats;
            self.slots[index].pending = true;
        }
    }

    fn poll(&mut self) {
        for slot in &mut self.slots {
            if !slot.pending {
                continue;
            }
            let mut available = 0;
            unsafe {
                gl::GetQueryObjectiv(slot.ids[5], gl::QUERY_RESULT_AVAILABLE, &mut available)
            };
            if available == 0 {
                continue;
            }
            let mut stamps = [0u64; QUERY_COUNT];
            unsafe {
                for (stamp, id) in stamps.iter_mut().zip(slot.ids) {
                    gl::GetQueryObjectui64v(id, gl::QUERY_RESULT, stamp);
                }
            }
            slot.pending = false;
            let stages = [
                stamps[1].saturating_sub(stamps[0]), // shadow
                stamps[2].saturating_sub(stamps[1]), // sky and setup
                stamps[3].saturating_sub(stamps[2]), // static Cell chunks
                stamps[4].saturating_sub(stamps[3]), // opaque Terrain
                stamps[5].saturating_sub(stamps[4]), // dynamic, water, particles
            ];
            for (sum, duration) in self.sums_ns.iter_mut().zip(stages) {
                *sum += duration as u128;
            }
            self.samples += 1;
            self.sums_draws.shadow_draws += slot.stats.shadow_draws;
            self.sums_draws.shadow_triangles += slot.stats.shadow_triangles;
            self.sums_draws.static_draws += slot.stats.static_draws;
            self.sums_draws.static_triangles += slot.stats.static_triangles;
            self.sums_draws.dynamic_draws += slot.stats.dynamic_draws;
            self.sums_draws.dynamic_triangles += slot.stats.dynamic_triangles;
            self.sums_draws.terrain_draws += slot.stats.terrain_draws;
            self.sums_draws.terrain_triangles += slot.stats.terrain_triangles;
            self.sums_draws.terrain_chunks_drawn += slot.stats.terrain_chunks_drawn;
            self.sums_draws.terrain_chunks_culled += slot.stats.terrain_chunks_culled;
            self.sums_draws.water_draws += slot.stats.water_draws;
            self.sums_draws.water_triangles += slot.stats.water_triangles;
        }
        if self.samples >= REPORT_SAMPLES {
            let ms = |index| self.sums_ns[index] as f64 / self.samples as f64 / 1_000_000.0;
            let draws = &self.sums_draws;
            eprintln!(
                "AEO GPU game ({n} samples): shadow {shadow:.3} ms, sky/setup {setup:.3} ms, static-cells {static_ms:.3} ms, terrain {terrain_ms:.3} ms, dynamic+water {dynamic:.3} ms, total {total:.3} ms | draws shadow/static/terrain/water/dynamic {sd}/{st}/{td}/{wd}/{dd}, triangles {stris}/{ttris}/{terrain_tris}/{water_tris}/{dtris} | terrain chunks drawn/culled {terrain_chunks}/{terrain_culled} per frame",
                n = self.samples,
                shadow = ms(0),
                setup = ms(1),
                static_ms = ms(2),
                terrain_ms = ms(3),
                dynamic = ms(4),
                total =
                    (self.sums_ns.iter().sum::<u128>() as f64) / self.samples as f64 / 1_000_000.0,
                sd = draws.shadow_draws / self.samples,
                st = draws.static_draws / self.samples,
                td = draws.terrain_draws / self.samples,
                wd = draws.water_draws / self.samples,
                dd = draws.dynamic_draws / self.samples,
                stris = draws.shadow_triangles / self.samples,
                ttris = draws.static_triangles / self.samples,
                terrain_tris = draws.terrain_triangles / self.samples,
                water_tris = draws.water_triangles / self.samples,
                dtris = draws.dynamic_triangles / self.samples,
                terrain_chunks = draws.terrain_chunks_drawn / self.samples,
                terrain_culled = draws.terrain_chunks_culled / self.samples,
            );
            self.samples = 0;
            self.sums_ns = [0; 5];
            self.sums_draws = DrawStats::default();
        }
    }
}

impl Drop for RenderProfiler {
    fn drop(&mut self) {
        unsafe {
            for slot in &self.slots {
                gl::DeleteQueries(QUERY_COUNT as i32, slot.ids.as_ptr());
            }
        }
    }
}
