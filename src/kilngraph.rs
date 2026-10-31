// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

use std::time::Instant;

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use std::cell::RefCell;

use std::collections::HashSet;

static FRAME: AtomicU64 = AtomicU64::new(0);

static MODE: AtomicU32 = AtomicU32::new(0);

static ACTIVITY: AtomicU32 = AtomicU32::new(0);

static TEXT_PACKETS: AtomicU32 = AtomicU32::new(0);

static SUPPRESSED: AtomicU64 = AtomicU64::new(0);

static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

static OUTPUT_COUNT: AtomicU32 = AtomicU32::new(0);

thread_local! { static METADATA: RefCell<HashSet<(u32,u64)>> = RefCell::new(HashSet::new());

static PROJECT: RefCell<u64> = const { RefCell::new(0) }; }

#[repr(C)]

struct Event {
    emitted_qpc: u64,
    duration_ns: u64,
    frame_id: u64,
    object_id: u64,
    script_id: u64,

    operation: u32,
    parent: u32,
    source_line: u32,
    flags: u32,
}

fn durations(points: &[Instant; 6]) -> Option<[u64; 6]> {
    let mut result = [0; 6];

    result[0] = points[5]
        .checked_duration_since(points[0])?
        .as_nanos()
        .min(u64::MAX as u128) as u64;

    for index in 0..5 {
        result[index + 1] = points[index + 1]
            .checked_duration_since(points[index])?
            .as_nanos()
            .min(u64::MAX as u128) as u64;
    }

    Some(result)
}

#[cfg(windows)]

mod windows_hook {

    use super::*;

    use std::{
        ffi::c_void,
        os::windows::ffi::OsStrExt,
        sync::{
            OnceLock,
            atomic::{AtomicU64, Ordering},
        },
    };

    #[link(name = "kernel32")]

    unsafe extern "system" {

        fn LoadLibraryExW(path: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;

        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;

        fn FreeLibrary(module: *mut c_void) -> i32;

    }

    type Start = unsafe extern "C" fn(u32) -> i32;

    type Emit = unsafe extern "C" fn(*const Event) -> i32;

    struct Producer {
        _module: usize,
        emit: Emit,
        frames: AtomicU64,
    }

    static PRODUCER: OnceLock<Option<Producer>> = OnceLock::new();

    fn load() -> Option<Producer> {
        let path = std::path::PathBuf::from(std::env::var_os("KILNGRAPH_DLL")?);

        if !path.is_absolute() || !path.is_file() {
            return None;
        }

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x100 | 0x800) };

        if module.is_null() {
            return None;
        }

        let start = unsafe { GetProcAddress(module, c"kg_start".as_ptr().cast()) };

        let emit = unsafe { GetProcAddress(module, c"kg_emit".as_ptr().cast()) };

        if start.is_null() || emit.is_null() {
            unsafe {
                FreeLibrary(module);
            }

            return None;
        }

        let start: Start = unsafe { std::mem::transmute(start) };

        if unsafe { start(1) } == 0 {
            unsafe {
                FreeLibrary(module);
            }

            return None;
        }

        Some(Producer {
            _module: module as usize,

            emit: unsafe { std::mem::transmute::<*mut c_void, Emit>(emit) },
            frames: AtomicU64::new(0),
        })
    }

    pub(super) fn enabled() -> bool {
        PRODUCER.get_or_init(load).is_some()
    }

    pub(super) fn emit(event: &Event) {
        if let Some(producer) = PRODUCER.get_or_init(load) {
            unsafe {
                (producer.emit)(event);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn drain_test_packets() -> Vec<u8> {
        let producer = PRODUCER.get().unwrap().as_ref().unwrap();
        let module = producer._module as *mut c_void;
        let open: unsafe extern "C" fn(u32, *mut u8) -> *mut c_void =
            unsafe { std::mem::transmute(GetProcAddress(module, c"kg_open".as_ptr().cast())) };
        let read: unsafe extern "C" fn(*mut c_void, *mut Event, u32, *mut u64) -> u32 =
            unsafe { std::mem::transmute(GetProcAddress(module, c"kg_read".as_ptr().cast())) };
        let close: unsafe extern "C" fn(*mut c_void) =
            unsafe { std::mem::transmute(GetProcAddress(module, c"kg_close".as_ptr().cast())) };
        let mut info = [0u64; 4];
        let reader = unsafe { open(std::process::id(), info.as_mut_ptr().cast()) };
        assert!(!reader.is_null());
        let mut events: Vec<Event> = Vec::with_capacity(4096);
        let mut loss = 0;
        let count = unsafe { read(reader, events.as_mut_ptr(), 4096, &mut loss) } as usize;
        assert_eq!(loss, 0);
        let raw = unsafe {
            std::slice::from_raw_parts(events.as_ptr().cast::<u8>(), count * 56).to_vec()
        };
        unsafe {
            close(reader);
        }
        raw
    }

    pub fn record(points: [Instant; 6], source_lines: [u32; 6]) {
        let Some(producer) = PRODUCER.get_or_init(load) else {
            return;
        };

        let Some(ns) = durations(&points) else {
            return;
        };

        let current = FRAME.load(Ordering::Relaxed);

        let frame_id = if current == 0 {
            producer.frames.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            current
        };

        for index in [1, 2, 3, 4, 5, 0] {
            let event = Event {
                emitted_qpc: 0,
                duration_ns: ns[index],
                frame_id,

                object_id: 0,
                script_id: 0,
                operation: index as u32 + 1,

                parent: if index == 0 { 0 } else { 1 },
                source_line: source_lines[index],

                flags: (if index == 0 { 1 } else { 0 }) | (MODE.load(Ordering::Relaxed) << 8),
            };

            unsafe {
                (producer.emit)(&event);
            }
        }
    }
}

pub fn enabled() -> bool {
    #[cfg(windows)]
    {
        windows_hook::enabled()
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn emit(event: &Event) {
    #[cfg(windows)]
    windows_hook::emit(event);

    #[cfg(not(windows))]
    let _ = event;
}

fn flags(kind: u32) -> u32 {
    kind | (MODE.load(Ordering::Relaxed) << 8)
}

fn hash(text: &str) -> u64 {
    text.bytes()
        .fold(14695981039346656037u64, |h, b| {
            (h ^ b as u64).wrapping_mul(1099511628211)
        })
        .max(1)
}

fn text(kind: u32, key: u64, script: u64, value: &str, cached: bool) {
    let mut length = value.len().min(256);

    while !value.is_char_boundary(length) {
        length -= 1;
    }

    let packets = ((length.max(1) + 15) / 16) as u32;

    if TEXT_PACKETS.load(Ordering::Relaxed).saturating_add(packets) > 512 {
        SUPPRESSED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    if cached {
        let skip = METADATA.with(|registry| {
            let mut registry = registry.borrow_mut();

            if registry.contains(&(kind, key)) {
                return true;
            }

            if registry.len() >= 1024 {
                SUPPRESSED.fetch_add(1, Ordering::Relaxed);
                return true;
            }

            registry.insert((kind, key));
            false
        });

        if skip {
            return;
        }
    }

    let bytes = &value.as_bytes()[..length];

    for offset in (0..length.max(1)).step_by(16) {
        if TEXT_PACKETS.fetch_add(1, Ordering::Relaxed) >= 512 {
            SUPPRESSED.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let count = (length - offset).min(16);

        let mut payload = [0u8; 16];

        payload[..count].copy_from_slice(&bytes[offset..offset + count]);

        emit(&Event {
            emitted_qpc: 0,
            duration_ns: u64::from_le_bytes(payload[..8].try_into().unwrap()),

            frame_id: u64::from_le_bytes(payload[8..].try_into().unwrap()),
            object_id: key,
            script_id: script,

            operation: count as u32,
            parent: offset as u32,
            source_line: 0,

            flags: flags(4) | (kind << 16) | if offset + count == length { 1 << 24 } else { 0 },
        });
    }
}

pub fn begin_frame(mode: u32, project: Option<&std::path::Path>) {
    if !enabled() {
        return;
    }

    FRAME.fetch_add(1, Ordering::Relaxed);

    MODE.store(mode.min(3), Ordering::Relaxed);

    ACTIVITY.store(0, Ordering::Relaxed);

    OUTPUT_COUNT.store(0, Ordering::Relaxed);

    TEXT_PACKETS.store(0, Ordering::Relaxed);

    let refresh = FRAME.load(Ordering::Relaxed) % 120 == 0;

    if refresh {
        METADATA.with(|registry| registry.borrow_mut().clear());
    }

    let lost = SUPPRESSED.swap(0, Ordering::Relaxed);

    if lost > 0 {
        emit(&Event {
            emitted_qpc: 0,
            duration_ns: lost,
            frame_id: 0,
            object_id: 0,
            script_id: 0,

            operation: 0,
            parent: 0,
            source_line: 0,
            flags: flags(5),
        });
    }

    let root = project.map(|p| p.to_string_lossy()).unwrap_or_default();

    let root_hash = hash(&root);

    PROJECT.with(|last| {
        if *last.borrow() != root_hash || refresh {
            text(3, 1, 0, &root, false);
            *last.borrow_mut() = root_hash;
        }
    });
}

pub struct Scope {
    started: Option<Instant>,
    operation: u32,
    line: u32,
    script: u64,
    object: u64,
    label: u64,
    kind: u32,
}

impl Scope {
    pub fn new(operation: u32, source: &str, line: u32) -> Self {
        let active = enabled();

        if active {
            text(5, operation as u64, 0, source, true);
        }

        Self {
            started: active.then(Instant::now),
            operation,
            line,
            script: 0,
            object: 0,
            label: 0,
            kind: 2,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            emit(&Event {
                emitted_qpc: 0,
                duration_ns: started.elapsed().as_nanos().min(u64::MAX as u128) as u64,

                frame_id: if self.kind == 3 {
                    self.label
                } else {
                    FRAME.load(Ordering::Relaxed)
                },

                object_id: self.object,
                script_id: self.script,
                operation: self.operation,

                parent: if self.kind == 3 {
                    FRAME.load(Ordering::Relaxed) as u32
                } else {
                    0
                },
                source_line: self.line,
                flags: flags(self.kind),
            });
        }
    }
}

pub fn script_scope(
    path: &str,
    function: &str,
    object: u64,
    action: u32,
    source_offset: u32,
) -> Scope {
    let active = enabled() && ACTIVITY.fetch_add(1, Ordering::Relaxed) < 96;

    let (script, label) = if active {
        let script = hash(path);

        let label = hash(function) ^ script;

        text(1, script, script, path, true);

        text(2, label, script, function, true);

        (script, label)
    } else {
        if enabled() {
            SUPPRESSED.fetch_add(1, Ordering::Relaxed);
        }
        (0, 0)
    };

    Scope {
        started: active.then(Instant::now),
        operation: action,
        line: source_offset,

        script,
        object,
        label,
        kind: 3,
    }
}

pub fn output(path: Option<&str>, message: &str, severity: &str, object: u64, line: u32) {
    if !enabled() {
        return;
    }

    if OUTPUT_COUNT.fetch_add(1, Ordering::Relaxed) >= 32 {
        SUPPRESSED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let path = path.unwrap_or("<runtime>");

    let script = hash(path);

    text(1, script, script, path, true);

    let id = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;

    text(4, id, script, message, false);

    let severity = match severity {
        "ERROR" => 1,
        "WARNING" => 2,
        "SYSTEM" => 3,
        _ => 0,
    };

    emit(&Event {
        emitted_qpc: 0,
        duration_ns: 0,
        frame_id: id,
        object_id: object,

        script_id: script,
        operation: severity,
        parent: 0,
        source_line: line,

        flags: flags(6),
    });
}

pub fn record_frame(points: [Instant; 6], source_lines: [u32; 6]) {
    #[cfg(windows)]
    windows_hook::record(points, source_lines);

    #[cfg(not(windows))]
    let _ = (points, source_lines);
}

#[cfg(test)]

mod tests {

    use super::*;

    use std::time::Duration;

    #[test]

    fn abi_matches_native_packet() {
        assert_eq!(std::mem::size_of::<Event>(), 56);
    }

    #[test]

    fn exact_existing_host_intervals_are_preserved() {
        let start = Instant::now();

        let points = [0, 2, 5, 9, 14, 20].map(|ms| start + Duration::from_millis(ms));

        assert_eq!(
            durations(&points),
            Some([
                20_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000, 6_000_000
            ])
        );
    }

    #[test]

    fn unordered_time_points_are_rejected() {
        let start = Instant::now();

        let points = [0, 2, 1, 9, 14, 20].map(|ms| start + Duration::from_millis(ms));

        assert_eq!(durations(&points), None);
    }
}

#[cfg(all(test, windows))]
mod runtime_probe {
    #[test]
    #[ignore = "Requires KILNGRAPH_DLL and KILNGRAPH_TEST_PACKETS; explicit native integration run"]
    fn actual_interpreter_emits_calls_and_output() {
        use crate::engine::entity::EntityManager;
        use crate::scripting::{
            api::HostContext, interpreter::Interpreter, lexer::Lexer, parser::Parser, value::Value,
        };
        super::begin_frame(3, None);
        assert!(super::enabled());
        let source = r#"entity Test {
            fn helper() { debug.log("KilnGraph real interpreter output") }
            fn update(dt: number) { helper() }
        }"#;
        let program = Parser::new(Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap();
        let mut interpreter = Interpreter::new(program);
        let mut engine = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.016,
            engine: &mut engine,
        };
        let mut instance = interpreter
            .instantiate_entity("Test", 42, &mut host)
            .unwrap();
        instance.script_path = Some("fixtures/live-probe.aeo".to_string());
        interpreter
            .call(
                &mut instance,
                "update",
                vec![Value::Number(0.016)],
                &mut host,
            )
            .unwrap();
        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(0.016)])
            .unwrap();
        let _ = interpreter.resume_fiber(&mut fiber, &mut host);
        let output = interpreter.drain_output();
        assert!(
            output
                .iter()
                .any(|r| r.message == "KilnGraph real interpreter output")
        );
        let now = std::time::Instant::now();
        super::record_frame([now; 6], [1; 6]);
        let raw = super::windows_hook::drain_test_packets();
        assert!(
            raw.chunks_exact(56)
                .any(|r| u32::from_le_bytes(r[52..56].try_into().unwrap()) & 255 == 3)
        );
        assert!(
            raw.chunks_exact(56)
                .any(|r| u32::from_le_bytes(r[52..56].try_into().unwrap()) & 255 == 6)
        );
        std::fs::write(std::env::var_os("KILNGRAPH_TEST_PACKETS").unwrap(), raw).unwrap();
    }
}
