use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, Networks, Pid, ProcessesToUpdate, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCore {
    pub id: u32,
    pub usage: f32,
    pub frequency: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySlot {
    pub slot: u32,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub memory_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub file_system: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub received_packets: u64,
    pub transmitted_packets: u64,
    pub errors_in: u32,
    pub errors_out: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: i32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub thread_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemLoad {
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp: i64,
    pub cpu_cores: Vec<CpuCore>,
    pub memory_slots: Vec<MemorySlot>,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInterface>,
    pub processes: Vec<ProcessInfo>,
    pub system_load: SystemLoad,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static RING_BUFFER: once_cell::sync::Lazy<std::sync::Mutex<Vec<Vec<u8>>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(Vec::with_capacity(16)));

fn collect() -> Vec<u8> {
    let mut sys = System::new_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let cpu_cores: Vec<CpuCore> = sys
        .cpus()
        .iter()
        .enumerate()
        .map(|(i, cpu)| CpuCore {
            id: i as u32,
            usage: cpu.cpu_usage(),
            frequency: cpu.frequency(),
            name: cpu.brand().to_string(),
        })
        .collect();

    let memory_slots = vec![MemorySlot {
        slot: 0,
        total: sys.total_memory(),
        used: sys.used_memory(),
        available: sys.free_memory(),
        memory_type: "Physical".to_string(),
    }];

    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();

    let disks: Vec<DiskInfo> = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().into_owned(),
            mount_point: d.mount_point().to_string_lossy().into_owned(),
            total: d.total_space(),
            used: d.total_space().saturating_sub(d.available_space()),
            available: d.available_space(),
            file_system: d.file_system().to_string_lossy().into_owned(),
        })
        .collect();

    let networks: Vec<NetworkInterface> = Networks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|(name, data)| NetworkInterface {
            name: name.clone(),
            received_bytes: data.total_received(),
            transmitted_bytes: data.total_transmitted(),
            received_packets: data.total_packets_received(),
            transmitted_packets: data.total_packets_transmitted(),
            errors_in: data.total_errors_on_received() as u32,
            errors_out: data.total_errors_on_transmitted() as u32,
        })
        .collect();

    let load_avg = System::load_average();
    let system_load = SystemLoad {
        load_1: load_avg.one,
        load_5: load_avg.five,
        load_15: load_avg.fifteen,
    };

    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessInfo {
            pid: pid.as_u32() as i32,
            name: p.name().to_string_lossy().into_owned(),
            cpu_usage: p.cpu_usage(),
            memory_bytes: p.memory(),
            thread_count: p.tasks()
                .map(|t| t.len() as u32)
                .unwrap_or(0),
        })
        .collect();
    processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap());
    let processes = processes.into_iter().take(20).collect();

    let snapshot = MetricsSnapshot {
        timestamp,
        cpu_cores,
        memory_slots,
        swap_total,
        swap_used,
        disks,
        networks,
        processes,
        system_load,
    };

    serde_json::to_vec(&snapshot).unwrap()
}

#[no_mangle]
pub extern "C" fn metrics_start_sampler(interval_ms: u64) {
    if RUNNING.load(Ordering::SeqCst) {
        return;
    }
    RUNNING.store(true, Ordering::SeqCst);
    thread::spawn(move || {
        while RUNNING.load(Ordering::SeqCst) {
            let data = collect();
            let mut ring = RING_BUFFER.lock().unwrap();
            if ring.len() >= 16 {
                ring.remove(0);
            }
            ring.push(data);
            drop(ring);
            thread::sleep(Duration::from_millis(interval_ms));
        }
    });
}

#[no_mangle]
pub extern "C" fn metrics_stop_sampler() {
    RUNNING.store(false, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn metrics_snapshot_size() -> i32 {
    let ring = RING_BUFFER.lock().unwrap();
    if let Some(data) = ring.last() {
        data.len() as i32
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn metrics_get_snapshot(buf: *mut u8, buf_len: i32) -> i32 {
    let ring = RING_BUFFER.lock().unwrap();
    if let Some(data) = ring.last() {
        let len = data.len() as i32;
        if buf_len < len {
            return -1;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf, len as usize);
        }
        len
    } else {
        -1
    }
}