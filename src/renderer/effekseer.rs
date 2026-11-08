// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::world::World;
use glam::{Mat4, Vec3};
use std::path::Path;
use std::time::Duration;

#[cfg(not(windows))]
pub(crate) fn set_profiling(_: bool) {}
#[cfg(not(windows))]
pub(crate) fn take_profile_duration() -> Option<Duration> {
    None
}
#[cfg(not(windows))]
pub(crate) fn take_profile_gpu_duration() -> Option<Duration> {
    None
}

#[cfg(not(windows))]
pub(super) fn draw(_: &World, _: Mat4, _: Mat4, _: Vec3, _: Option<&Path>) -> bool {
    false
}
#[cfg(not(windows))]
pub(super) fn shutdown() {}
#[cfg(not(windows))]
pub(crate) fn set_preview(_: Option<&Path>, _: bool, _: u64) {}
#[cfg(not(windows))]
pub(crate) fn invalidate_project_fx_asset(_: &Path) {}

#[cfg(windows)]
mod windows_runtime {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::{BTreeMap, HashSet};
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::rc::{Rc, Weak};
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Instant;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(path: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, symbol: *const u8) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }
    #[repr(C)]
    struct FireEmitter {
        id: u64,
        x: f32,
        y: f32,
        z: f32,
        scale: f32,
        rate: f32,
        radius: f32,
    }
    type Create = unsafe extern "C" fn(*const u16) -> *mut c_void;
    type Draw = unsafe extern "C" fn(*mut c_void, *const f32, *const f32, *const f32, f32);
    type Sync = unsafe extern "C" fn(*mut c_void, *const FireEmitter, u32);
    type Destroy = unsafe extern "C" fn(*mut c_void);
    type LastError = unsafe extern "C" fn() -> *const i8;

    struct Native {
        library: *mut c_void,
        instance: *mut c_void,
        draw: Draw,
        sync: Sync,
        destroy: Destroy,
        last_frame: Instant,
        resources: Rc<SharedFxResources>,
    }

    #[derive(Default)]
    struct PreviewState {
        requested: Option<PathBuf>,
        revision: u64,
        current_revision: u64,
        loaded_path: Option<PathBuf>,
        playing: bool,
        native: Option<Native>,
        failed: bool,
    }
    pub(crate) fn set_preview(path: Option<&Path>, playing: bool, revision: u64) {
        PREVIEW_STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.requested = path.map(Path::to_path_buf);
            state.playing = playing;
            state.revision = revision;
        });
    }
    static FX_INVALIDATIONS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static FX_INVALIDATION_PENDING: AtomicBool = AtomicBool::new(false);
    pub(crate) fn invalidate_project_fx_asset(path: &Path) {
        let Ok(absolute) = path.canonicalize() else {
            return;
        };
        if let Ok(mut queue) = FX_INVALIDATIONS.lock() {
            if !queue.contains(&absolute) {
                queue.push(absolute);
            }
            FX_INVALIDATION_PENDING.store(true, Ordering::Release);
        }
    }
    fn take_fx_invalidations() -> Vec<PathBuf> {
        if !FX_INVALIDATION_PENDING.swap(false, Ordering::AcqRel) {
            return Vec::new();
        }
        FX_INVALIDATIONS
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    thread_local! {
        static PROFILING: Cell<bool> = const { Cell::new(false) };
        static PROFILE_DURATION: Cell<Option<Duration>> = const { Cell::new(None) };
        static PROFILE_GPU_DURATION: Cell<Option<Duration>> = const { Cell::new(None) };
        static GPU_TIMER: RefCell<AsyncGpuTimer> = RefCell::new(AsyncGpuTimer::default());
        static ENGINE_RESOURCES: RefCell<Weak<SharedFxResources>> = RefCell::new(Weak::new());
        static FX_STATE: RefCell<FxState> = RefCell::new(FxState::default());
        static PREVIEW_STATE: RefCell<PreviewState> = RefCell::new(PreviewState::default());
    }
    #[derive(Default)]
    struct GpuQueryPair {
        begin: u32,
        end: u32,
        pending: bool,
    }
    #[derive(Default)]
    struct AsyncGpuTimer {
        queries: [GpuQueryPair; 3],
        active: Option<usize>,
        cursor: usize,
    }
    impl AsyncGpuTimer {
        fn available() -> bool {
            gl::GenQueries::is_loaded()
                && gl::QueryCounter::is_loaded()
                && gl::GetQueryObjectiv::is_loaded()
                && gl::GetQueryObjectui64v::is_loaded()
        }
        fn poll(&mut self) -> Option<Duration> {
            if !Self::available() {
                return None;
            }
            let mut latest = None;
            for pair in &mut self.queries {
                if !pair.pending {
                    continue;
                }
                let mut available = 0_i32;
                unsafe {
                    gl::GetQueryObjectiv(pair.end, gl::QUERY_RESULT_AVAILABLE, &mut available);
                }
                if available == 0 {
                    continue;
                }
                let (mut begin, mut end) = (0_u64, 0_u64);
                unsafe {
                    gl::GetQueryObjectui64v(pair.begin, gl::QUERY_RESULT, &mut begin);
                    gl::GetQueryObjectui64v(pair.end, gl::QUERY_RESULT, &mut end);
                }
                pair.pending = false;
                if end >= begin {
                    latest = Some(Duration::from_nanos(end - begin));
                }
            }
            latest
        }
        fn begin(&mut self) {
            if !Self::available() {
                return;
            }
            for step in 0..self.queries.len() {
                let i = (self.cursor + step) % self.queries.len();
                let slot = &mut self.queries[i];
                if slot.pending {
                    continue;
                }
                if slot.begin == 0 || slot.end == 0 {
                    let mut ids = [0_u32; 2];
                    unsafe {
                        gl::GenQueries(2, ids.as_mut_ptr());
                    }
                    if ids[0] == 0 || ids[1] == 0 {
                        return;
                    }
                    slot.begin = ids[0];
                    slot.end = ids[1];
                }
                unsafe {
                    gl::QueryCounter(slot.begin, gl::TIMESTAMP);
                }
                self.active = Some(i);
                self.cursor = (i + 1) % self.queries.len();
                return;
            }
        }
        fn end(&mut self) {
            let Some(i) = self.active.take() else {
                return;
            };
            unsafe {
                gl::QueryCounter(self.queries[i].end, gl::TIMESTAMP);
            }
            self.queries[i].pending = true;
        }
        fn cleanup(&mut self) {
            if gl::DeleteQueries::is_loaded() {
                for pair in &mut self.queries {
                    let ids = [pair.begin, pair.end];
                    unsafe {
                        gl::DeleteQueries(2, ids.as_ptr());
                    }
                    *pair = GpuQueryPair::default();
                }
            }
            self.active = None;
        }
    }

    const MAX_NATIVE_FX_ASSETS: usize = 64;
    const MAX_NATIVE_FX_EMITTERS: usize = 256;

    struct FxState {
        by_asset: BTreeMap<String, Native>,
        unavailable: HashSet<String>,
        project_root: Option<PathBuf>,
        requested_root: Option<PathBuf>,
        overflow_logged: bool,
    }
    impl Default for FxState {
        fn default() -> Self {
            Self {
                by_asset: BTreeMap::new(),
                unavailable: HashSet::new(),
                project_root: None,
                requested_root: None,
                overflow_logged: false,
            }
        }
    }
    impl FxState {
        fn switch_project(&mut self, root: Option<&Path>) -> bool {
            if self.requested_root.as_deref() == root {
                return false;
            }
            self.by_asset.clear();
            self.unavailable.clear();
            self.requested_root = root.map(Path::to_path_buf);
            self.project_root = root.and_then(|path| path.canonicalize().ok());
            self.overflow_logged = false;
            true
        }
    }
    fn limit_project_fx(
        grouped: BTreeMap<String, Vec<FireEmitter>>,
    ) -> (BTreeMap<String, Vec<FireEmitter>>, usize) {
        let skipped = grouped.len().saturating_sub(MAX_NATIVE_FX_ASSETS);
        (
            grouped.into_iter().take(MAX_NATIVE_FX_ASSETS).collect(),
            skipped,
        )
    }

    unsafe fn symbol<T>(library: *mut c_void, name: &'static [u8]) -> Option<T> {
        let address = unsafe { GetProcAddress(library, name.as_ptr()) };
        if address.is_null() {
            return None;
        }
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&address) })
    }

    fn unpack_engine_resources() -> Result<PathBuf, String> {
        use std::io::Write;
        const RESOURCES: &[(&str, &[u8])] = &[
            (
                "WorldkilnEffekseerPoc.dll",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/WorldkilnEffekseerPoc.dll"
                )),
            ),
            (
                "fire.efkefc",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/fire.efkefc"
                )),
            ),
            (
                "Texture/fire_tex.png",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/Texture/fire_tex.png"
                )),
            ),
            (
                "Texture/smoke_tex.png",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/Texture/smoke_tex.png"
                )),
            ),
            (
                "Texture/glow.png",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/Texture/glow.png"
                )),
            ),
            (
                "Texture/double-shockwave.png",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/Texture/double-shockwave.png"
                )),
            ),
            (
                "LICENSE_EFFEKSEER.txt",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/LICENSE_EFFEKSEER.txt"
                )),
            ),
            (
                "LICENSE_EFFECTS.txt",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/assets/effekseer/LICENSE_EFFECTS.txt"
                )),
            ),
        ];
        let mut destination = None;
        for _ in 0..8 {
            let path = std::env::temp_dir().join(format!(
                "worldkiln-fire-{}-{:016x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            if std::fs::create_dir(&path).is_ok() {
                destination = Some(path);
                break;
            }
        }
        let path = destination.ok_or("Unable to create isolated Effekseer resource directory")?;
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir(path.join("Texture"))?;
            for (relative, bytes) in RESOURCES {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path.join(relative))?;
                file.write_all(bytes)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_dir_all(&path);
            return Err(format!("Unable to unpack Effekseer resources: {error}"));
        }
        Ok(path)
    }

    struct SharedFxResources {
        root: PathBuf,
    }
    impl Drop for SharedFxResources {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn shared_engine_resources() -> Result<Rc<SharedFxResources>, String> {
        ENGINE_RESOURCES.with(|slot| {
            if let Some(existing) = slot.borrow().upgrade() {
                return Ok(existing);
            }
            let fresh = Rc::new(SharedFxResources {
                root: unpack_engine_resources()?,
            });
            *slot.borrow_mut() = Rc::downgrade(&fresh);
            Ok(fresh)
        })
    }

    impl Native {
        fn open(effect: Option<&Path>) -> Result<Self, String> {
            use std::os::windows::ffi::OsStrExt;
            let resources = shared_engine_resources()?;
            let dll_path = resources.root.join("WorldkilnEffekseerPoc.dll");
            let effect_path = effect
                .map(Path::to_path_buf)
                .unwrap_or_else(|| resources.root.join("fire.efkefc"));
            let wide = |path: &std::path::Path| -> Vec<u16> {
                path.as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect()
            };
            let library = unsafe { LoadLibraryW(wide(&dll_path).as_ptr()) };
            if library.is_null() {
                return Err("Effekseer DLL could not be loaded".to_string());
            }
            let result = (|| {
                let create: Create = unsafe { symbol(library, b"wkfx_create\0") }
                    .ok_or("Effekseer create export missing")?;
                let draw: Draw = unsafe { symbol(library, b"wkfx_draw\0") }
                    .ok_or("Effekseer draw export missing")?;
                let sync: Sync = unsafe { symbol(library, b"wkfx_sync_fire\0") }
                    .ok_or("Effekseer emitter sync export missing")?;
                let destroy: Destroy = unsafe { symbol(library, b"wkfx_destroy\0") }
                    .ok_or("Effekseer destroy export missing")?;
                let last_error: LastError = unsafe { symbol(library, b"wkfx_last_error\0") }
                    .ok_or("Effekseer diagnostics export missing")?;
                let instance = unsafe { create(wide(&effect_path).as_ptr()) };
                if instance.is_null() {
                    let message = unsafe {
                        let ptr = last_error();
                        if ptr.is_null() {
                            "No error description".into()
                        } else {
                            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
                        }
                    };
                    return Err(format!("Unable to load Effekseer fire: {message}"));
                }
                Ok(Self {
                    library,
                    instance,
                    draw,
                    sync,
                    destroy,
                    last_frame: Instant::now(),
                    resources: Rc::clone(&resources),
                })
            })();
            if result.is_err() {
                unsafe {
                    FreeLibrary(library);
                }
            }
            result
        }
    }

    impl Drop for Native {
        fn drop(&mut self) {
            unsafe {
                (self.destroy)(self.instance);
                FreeLibrary(self.library);
            }
        }
    }

    pub(crate) fn set_profiling(enabled: bool) {
        PROFILING.with(|state| state.set(enabled));
        PROFILE_DURATION.with(|state| state.set(None));
        PROFILE_GPU_DURATION.with(|state| state.set(None));
    }

    pub(crate) fn take_profile_duration() -> Option<Duration> {
        PROFILE_DURATION.with(|state| state.take())
    }
    pub(crate) fn take_profile_gpu_duration() -> Option<Duration> {
        PROFILE_GPU_DURATION.with(|state| state.take())
    }

    pub(crate) fn draw(
        world: &World,
        view: Mat4,
        projection: Mat4,
        camera: Vec3,
        project_root: Option<&Path>,
    ) -> bool {
        let started = PROFILING.with(|state| state.get().then(Instant::now));
        let fx = world.fx_emitter_samples();
        if started.is_some() {
            let has_fx = (project_root.is_some() && !fx.is_empty())
                || PREVIEW_STATE.with(|state| state.borrow().requested.is_some());
            GPU_TIMER.with(|timer| {
                let mut timer = timer.borrow_mut();
                if let Some(ready) = timer.poll() {
                    PROFILE_GPU_DURATION.with(|cell| cell.set(Some(ready)));
                }
                if has_fx {
                    timer.begin();
                }
            });
        }
        FX_STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            state.switch_project(project_root);
        });
        if let Some(root) = project_root {
            let mut grouped: BTreeMap<String, Vec<FireEmitter>> = BTreeMap::new();
            for (asset, id, pos, scale, rate, radius) in fx.into_iter().take(MAX_NATIVE_FX_EMITTERS)
            {
                grouped.entry(asset).or_default().push(FireEmitter {
                    id,
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    scale,
                    rate,
                    radius,
                });
            }
            let (grouped, dropped_assets) = limit_project_fx(grouped);
            let invalidated = take_fx_invalidations();
            FX_STATE.with(|state|{
                let mut state=state.borrow_mut();
                if dropped_assets>0 {
                    if !state.overflow_logged {
                        crate::engine::diagnostics::report("WARNING","Effects",
                            format!("Native FX asset limit {MAX_NATIVE_FX_ASSETS} reached; {dropped_assets} distinct effect assets skipped"));
                        state.overflow_logged=true;
                    }
                } else {
                    state.overflow_logged=false;
                }
                state.unavailable.retain(|id|grouped.contains_key(id));
                if !invalidated.is_empty() {
                    let changed:Vec<String>=grouped.keys().filter(|id| {
                        crate::project::fx_assets::resolve(root,id)
                            .is_some_and(|path|invalidated.contains(&path))
                    }).cloned().collect();
                    for id in changed {
                        state.by_asset.remove(&id);
                        state.unavailable.remove(&id);
                    }
                }
                state.by_asset.retain(|asset,_|grouped.contains_key(asset));
                for (id,emitters) in &grouped {
                    if !state.by_asset.contains_key(id) && !state.unavailable.contains(id) {
                        let Some(path)=crate::project::fx_assets::resolve(root,id) else {
                            state.unavailable.insert(id.clone());
                            crate::engine::diagnostics::report("WARNING","Effects",format!("Missing project FX: {id}"));
                            continue;
                        };
                        match Native::open(Some(&path)) {
                            Ok(native)=>{state.by_asset.insert(id.clone(),native);},
                            Err(error)=>{
                                state.unavailable.insert(id.clone());
                                crate::engine::diagnostics::report("ERROR","Effects",format!("{id}: {error}"));
                            },
                        }
                    }
                    let Some(native)=state.by_asset.get_mut(id) else {continue;};
                    unsafe { (native.sync)(native.instance,emitters.as_ptr(),emitters.len() as u32); }
                    let now=Instant::now();
                    let delta=now.duration_since(native.last_frame).as_secs_f32().min(0.05);
                    native.last_frame=now;
                    let view=view.to_cols_array();
                    let projection=projection.to_cols_array();
                    let camera=camera.to_array();
                    unsafe {(native.draw)(native.instance,view.as_ptr(),projection.as_ptr(),camera.as_ptr(),delta);}
                }
            });
        }
        PREVIEW_STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            let requested = state.requested.clone();
            if requested.is_none() {
                state.native = None;
                state.loaded_path = None;
                state.failed = false;
            } else {
                if state.loaded_path != requested || state.current_revision != state.revision {
                    state.native = None;
                    state.loaded_path = requested.clone();
                    state.current_revision = state.revision;
                    state.failed = false;
                }
                if state.native.is_none() && !state.failed {
                    state.native =
                        requested
                            .as_deref()
                            .and_then(|path| match Native::open(Some(path)) {
                                Ok(native) => Some(native),
                                Err(error) => {
                                    state.failed = true;
                                    crate::engine::diagnostics::report("ERROR", "AeoFX", error);
                                    None
                                }
                            });
                }
                let playing = state.playing;
                if let Some(native) = state.native.as_mut() {
                    let center = view.inverse().transform_point3(Vec3::new(0.0, 0.0, -5.0));
                    let sample = FireEmitter {
                        id: 1,
                        x: center.x,
                        y: center.y,
                        z: center.z,
                        scale: 1.0,
                        rate: 1.0,
                        radius: 0.5,
                    };
                    if playing {
                        unsafe {
                            (native.sync)(native.instance, &sample, 1);
                        }
                    }
                    let now = Instant::now();
                    let delta = if playing {
                        now.duration_since(native.last_frame)
                            .as_secs_f32()
                            .min(0.05)
                    } else {
                        0.0
                    };
                    native.last_frame = now;
                    let a = view.to_cols_array();
                    let b = projection.to_cols_array();
                    let c = camera.to_array();
                    unsafe {
                        (native.draw)(native.instance, a.as_ptr(), b.as_ptr(), c.as_ptr(), delta);
                    }
                }
            }
        });
        if let Some(started) = started {
            GPU_TIMER.with(|timer| timer.borrow_mut().end());
            PROFILE_DURATION.with(|state| state.set(Some(started.elapsed())));
        }
        false
    }

    pub(crate) fn shutdown() {
        GPU_TIMER.with(|timer| timer.borrow_mut().cleanup());
        FX_STATE.with(|cell| *cell.borrow_mut() = FxState::default());
        PREVIEW_STATE.with(|cell| *cell.borrow_mut() = PreviewState::default());
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn shared_native_resources_are_reused_and_reclaimed() {
            let first = shared_engine_resources().unwrap();
            let base = first.root.clone();
            assert!(base.join("WorldkilnEffekseerPoc.dll").is_file());
            let second = shared_engine_resources().unwrap();
            assert!(Rc::ptr_eq(&first, &second));
            drop(first);
            assert!(base.is_dir(), "resources remain until last consumer");
            drop(second);
            assert!(!base.exists(), "all resources are reclaimed on last drop");
            let third = shared_engine_resources().unwrap();
            assert_ne!(third.root, base, "fresh allocation after release");
            drop(third);
        }
        #[test]
        fn native_asset_limits_are_deterministic_and_project_cache_is_bounded() {
            let mut groups = BTreeMap::new();
            for i in (0..MAX_NATIVE_FX_ASSETS + 20).rev() {
                groups.insert(
                    format!("FX{i:03}.efkefc"),
                    vec![FireEmitter {
                        id: i as u64,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        scale: 1.0,
                        rate: 20.0,
                        radius: 0.4,
                    }],
                );
            }
            let (accepted, skipped) = limit_project_fx(groups);
            assert_eq!(accepted.len(), MAX_NATIVE_FX_ASSETS);
            assert_eq!(skipped, 20);
            assert!(accepted.contains_key("FX000.efkefc"));
            assert!(!accepted.contains_key("FX083.efkefc"));
            assert!(FxState::default().project_root.is_none());
            let base =
                std::env::temp_dir().join(format!("wk-fx-switch-{:032x}", rand::random::<u128>()));
            let a = base.join("project_a");
            let b = base.join("project_b");
            std::fs::create_dir_all(&a).unwrap();
            std::fs::create_dir_all(&b).unwrap();
            let mut frame = FxState::default();
            assert!(frame.switch_project(Some(&a)));
            frame.unavailable.insert("FX001.efkefc".into());
            assert!(
                !frame.switch_project(Some(&a)),
                "same project must not rescan filesystem"
            );
            assert_eq!(frame.unavailable.len(), 1);
            assert!(frame.switch_project(Some(&b)));
            assert!(
                frame.unavailable.is_empty(),
                "the next project must never inherit absent FX cache"
            );
            assert_eq!(frame.project_root, Some(b.canonicalize().unwrap()));
            assert!(frame.switch_project(None));
            assert!(frame.project_root.is_none());
            std::fs::remove_dir_all(base).unwrap();
        }

        #[test]
        fn profiling_is_opt_in_and_consumes_samples_only_once() {
            set_profiling(false);
            assert_eq!(take_profile_duration(), None);
            assert_eq!(take_profile_gpu_duration(), None);
            set_profiling(true);
            assert_eq!(take_profile_duration(), None);
            PROFILE_DURATION.with(|slot| slot.set(Some(Duration::from_millis(3))));
            assert_eq!(take_profile_duration(), Some(Duration::from_millis(3)));
            assert_eq!(take_profile_duration(), None);
            PROFILE_DURATION.with(|slot| slot.set(Some(Duration::from_secs(1))));
            set_profiling(false);
            assert_eq!(take_profile_duration(), None);
        }

        #[test]
        fn bundled_fire_effect_is_available_without_an_external_install() {
            let directory = unpack_engine_resources().expect("extract built-in effect");
            assert!(directory.join("fire.efkefc").is_file());
            assert!(directory.join("WorldkilnEffekseerPoc.dll").is_file());
            assert!(directory.join("Texture/fire_tex.png").is_file());
            assert!(directory.join("LICENSE_EFFEKSEER.txt").is_file());
            assert!(directory.join("LICENSE_EFFECTS.txt").is_file());
            std::fs::remove_dir_all(directory).expect("remove isolated test files");
        }
    }
}

#[cfg(windows)]
pub(super) use windows_runtime::{draw, shutdown};
#[cfg(windows)]
pub(crate) use windows_runtime::{
    invalidate_project_fx_asset, set_preview, set_profiling, take_profile_duration,
    take_profile_gpu_duration,
};
