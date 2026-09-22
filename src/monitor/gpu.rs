use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[derive(Debug, Clone, Default)]
pub struct GpuData {
    pub name: String,
    pub usage: Option<f32>,
    pub temperature: Option<f32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
}

pub trait GpuProvider: Send {
    fn sample(&mut self) -> Option<GpuData>;
}

// ----------------------------------------------------------------------------
// NVIDIA NVML Provider
// ----------------------------------------------------------------------------

#[repr(C)]
#[derive(Default)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

type NvmlReturn = u32;
type NvmlDevice = *mut c_void;

type FnNvmlInit = unsafe extern "C" fn() -> NvmlReturn;
type FnNvmlShutdown = unsafe extern "C" fn() -> NvmlReturn;
type FnNvmlDeviceGetCount = unsafe extern "C" fn(*mut u32) -> NvmlReturn;
type FnNvmlDeviceGetHandleByIndex = unsafe extern "C" fn(u32, *mut NvmlDevice) -> NvmlReturn;
type FnNvmlDeviceGetName = unsafe extern "C" fn(NvmlDevice, *mut u8, u32) -> NvmlReturn;
type FnNvmlDeviceGetUtilizationRates = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> NvmlReturn;
type FnNvmlDeviceGetTemperature = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> NvmlReturn;
type FnNvmlDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> NvmlReturn;

pub struct NvmlGpuProvider {
    hmodule: HMODULE,
    device: NvmlDevice,
    device_name: String,
    fn_get_util: FnNvmlDeviceGetUtilizationRates,
    fn_get_temp: FnNvmlDeviceGetTemperature,
    fn_get_mem: FnNvmlDeviceGetMemoryInfo,
    fn_shutdown: Option<FnNvmlShutdown>,
}

unsafe impl Send for NvmlGpuProvider {}

impl NvmlGpuProvider {
    pub fn try_new() -> Option<Self> {
        let dll_name: Vec<u16> = "nvml.dll\0".encode_utf16().collect();
        let hmodule = unsafe { LoadLibraryW(PCWSTR(dll_name.as_ptr())) }.ok()?;

        unsafe {
            let init_sym = GetProcAddress(hmodule, windows::core::s!("nvmlInit_v2"))
                .or_else(|| GetProcAddress(hmodule, windows::core::s!("nvmlInit")))?;
            let init: FnNvmlInit = std::mem::transmute(init_sym);
            if init() != 0 {
                let _ = FreeLibrary(hmodule);
                return None;
            }

            let shutdown_sym = GetProcAddress(hmodule, windows::core::s!("nvmlShutdown"));
            let fn_shutdown: Option<FnNvmlShutdown> = shutdown_sym.map(|s| std::mem::transmute(s));

            let get_count_sym = GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetCount_v2"))
                .or_else(|| GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetCount")))?;
            let get_handle_sym = GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetHandleByIndex_v2"))
                .or_else(|| GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetHandleByIndex")))?;
            let get_name_sym = GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetName"))?;
            let get_util_sym = GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetUtilizationRates"))?;
            let get_temp_sym = GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetTemperature"))?;
            let get_mem_sym = GetProcAddress(hmodule, windows::core::s!("nvmlDeviceGetMemoryInfo"))?;

            let get_count: FnNvmlDeviceGetCount = std::mem::transmute(get_count_sym);
            let get_handle: FnNvmlDeviceGetHandleByIndex = std::mem::transmute(get_handle_sym);
            let get_name: FnNvmlDeviceGetName = std::mem::transmute(get_name_sym);
            let fn_get_util: FnNvmlDeviceGetUtilizationRates = std::mem::transmute(get_util_sym);
            let fn_get_temp: FnNvmlDeviceGetTemperature = std::mem::transmute(get_temp_sym);
            let fn_get_mem: FnNvmlDeviceGetMemoryInfo = std::mem::transmute(get_mem_sym);

            let mut count = 0u32;
            if get_count(&mut count) != 0 || count == 0 {
                if let Some(sd) = fn_shutdown {
                    sd();
                }
                let _ = FreeLibrary(hmodule);
                return None;
            }

            // Pick device 0 (primary GPU)
            let mut device: NvmlDevice = std::ptr::null_mut();
            if get_handle(0, &mut device) != 0 {
                if let Some(sd) = fn_shutdown {
                    sd();
                }
                let _ = FreeLibrary(hmodule);
                return None;
            }

            let mut name_buf = [0u8; 96];
            let device_name = if get_name(device, name_buf.as_mut_ptr(), name_buf.len() as u32) == 0 {
                let len = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
                String::from_utf8_lossy(&name_buf[..len]).trim().to_string()
            } else {
                "NVIDIA GPU".to_string()
            };

            Some(Self {
                hmodule,
                device,
                device_name,
                fn_get_util,
                fn_get_temp,
                fn_get_mem,
                fn_shutdown,
            })
        }
    }
}

impl Drop for NvmlGpuProvider {
    fn drop(&mut self) {
        unsafe {
            if let Some(shutdown) = self.fn_shutdown {
                shutdown();
            }
            let _ = FreeLibrary(self.hmodule);
        }
    }
}

impl GpuProvider for NvmlGpuProvider {
    fn sample(&mut self) -> Option<GpuData> {
        unsafe {
            let mut util = NvmlUtilization::default();
            let usage = if (self.fn_get_util)(self.device, &mut util) == 0 {
                Some(util.gpu as f32)
            } else {
                None
            };

            let mut temp = 0u32;
            let temperature = if (self.fn_get_temp)(self.device, 0, &mut temp) == 0 {
                Some(temp as f32)
            } else {
                None
            };

            let mut mem = NvmlMemory::default();
            let (memory_used, memory_total) = if (self.fn_get_mem)(self.device, &mut mem) == 0 {
                (Some(mem.used), Some(mem.total))
            } else {
                (None, None)
            };

            Some(GpuData {
                name: self.device_name.clone(),
                usage,
                temperature,
                memory_used,
                memory_total,
            })
        }
    }
}

// ----------------------------------------------------------------------------
// Generic GPU Provider
// ----------------------------------------------------------------------------

pub struct GenericGpuProvider {
    name: String,
    vram_bytes: Option<u64>,
}

impl GenericGpuProvider {
    pub fn try_new() -> Option<Self> {
        Some(Self {
            name: "Generic / Integrated GPU".to_string(),
            vram_bytes: None,
        })
    }
}

impl GpuProvider for GenericGpuProvider {
    fn sample(&mut self) -> Option<GpuData> {
        Some(GpuData {
            name: self.name.clone(),
            usage: None,
            temperature: None,
            memory_used: None,
            memory_total: self.vram_bytes,
        })
    }
}

// ----------------------------------------------------------------------------
// Composite GPU Monitor
// ----------------------------------------------------------------------------

pub struct GpuMonitor {
    provider: Option<Box<dyn GpuProvider>>,
}

impl GpuMonitor {
    pub fn new() -> Self {
        let provider: Option<Box<dyn GpuProvider>> = if let Some(nvml) = NvmlGpuProvider::try_new() {
            Some(Box::new(nvml))
        } else if let Some(generic) = GenericGpuProvider::try_new() {
            Some(Box::new(generic))
        } else {
            None
        };

        Self { provider }
    }

    pub fn sample(&mut self) -> Option<GpuData> {
        if let Some(provider) = &mut self.provider {
            provider.sample()
        } else {
            None
        }
    }
}
