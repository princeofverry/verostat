use sysinfo::Components;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type PdhHQuery = usize;
type PdhHCounter = usize;
type PdhStatus = u32;

const PDH_FMT_DOUBLE: u32 = 0x00000200;

#[repr(C)]
#[derive(Default)]
struct PdhFmtCountervalue {
    c_status: u32,
    padding: u32,
    double_value: f64,
}

type FnPdhOpenQueryW = unsafe extern "system" fn(PCWSTR, usize, *mut PdhHQuery) -> PdhStatus;
type FnPdhAddEnglishCounterW = unsafe extern "system" fn(PdhHQuery, PCWSTR, usize, *mut PdhHCounter) -> PdhStatus;
type FnPdhCollectQueryData = unsafe extern "system" fn(PdhHQuery) -> PdhStatus;
type FnPdhGetFormattedCounterValue = unsafe extern "system" fn(PdhHCounter, u32, *mut u32, *mut PdhFmtCountervalue) -> PdhStatus;
type FnPdhCloseQuery = unsafe extern "system" fn(PdhHQuery) -> PdhStatus;

struct PdhThermalReader {
    hmodule: HMODULE,
    query: PdhHQuery,
    counter: PdhHCounter,
    is_high_precision: bool,
    fn_collect: FnPdhCollectQueryData,
    fn_get_val: FnPdhGetFormattedCounterValue,
    fn_close: FnPdhCloseQuery,
}

unsafe impl Send for PdhThermalReader {}

impl PdhThermalReader {
    pub fn try_new() -> Option<Self> {
        unsafe {
            let hmodule = LoadLibraryW(w!("pdh.dll")).ok()?;

            let open_sym = GetProcAddress(hmodule, windows::core::s!("PdhOpenQueryW"))?;
            let add_sym = GetProcAddress(hmodule, windows::core::s!("PdhAddEnglishCounterW"))?;
            let collect_sym = GetProcAddress(hmodule, windows::core::s!("PdhCollectQueryData"))?;
            let get_val_sym = GetProcAddress(hmodule, windows::core::s!("PdhGetFormattedCounterValue"))?;
            let close_sym = GetProcAddress(hmodule, windows::core::s!("PdhCloseQuery"))?;

            let pdh_open: FnPdhOpenQueryW = std::mem::transmute(open_sym);
            let pdh_add: FnPdhAddEnglishCounterW = std::mem::transmute(add_sym);
            let pdh_collect: FnPdhCollectQueryData = std::mem::transmute(collect_sym);
            let pdh_get_val: FnPdhGetFormattedCounterValue = std::mem::transmute(get_val_sym);
            let pdh_close: FnPdhCloseQuery = std::mem::transmute(close_sym);

            let mut query: PdhHQuery = 0;
            if pdh_open(PCWSTR::null(), 0, &mut query) != 0 {
                let _ = FreeLibrary(hmodule);
                return None;
            }

            // Candidates for CPU thermal zone on Windows
            let candidates = [
                ("\\Thermal Zone Information(\\_TZ.TZ00)\\Temperature", false),
                ("\\Thermal Zone Information(\\_TZ.TZ01)\\Temperature", false),
                ("\\Thermal Zone Information(*)\\Temperature", false),
                ("\\Thermal Zone Information(\\_TZ.TZ00)\\High Precision Temperature", true),
                ("\\Thermal Zone Information(*)\\High Precision Temperature", true),
            ];

            let mut active_counter: PdhHCounter = 0;
            let mut is_high_precision = false;

            for (cand, high_prec) in candidates {
                let wide: Vec<u16> = cand.encode_utf16().chain(std::iter::once(0)).collect();
                let mut counter: PdhHCounter = 0;
                if pdh_add(query, PCWSTR(wide.as_ptr()), 0, &mut counter) == 0 {
                    // Test collect
                    if pdh_collect(query) == 0 {
                        let mut val = PdhFmtCountervalue::default();
                        if pdh_get_val(counter, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut val) == 0 {
                            let kelvin = if high_prec {
                                val.double_value / 10.0
                            } else {
                                val.double_value
                            };
                            let celsius = kelvin - 273.15;
                            // Plausible CPU operating range: 10°C to 125°C.
                            // Values <= 5°C indicate an uninitialized ACPI thermal zone stub (e.g. 0°C / 273.15K).
                            if celsius >= 10.0 && celsius <= 125.0 {
                                active_counter = counter;
                                is_high_precision = high_prec;
                                break;
                            }
                        }
                    }
                }
            }

            if active_counter == 0 {
                let _ = pdh_close(query);
                let _ = FreeLibrary(hmodule);
                return None;
            }

            Some(Self {
                hmodule,
                query,
                counter: active_counter,
                is_high_precision,
                fn_collect: pdh_collect,
                fn_get_val: pdh_get_val,
                fn_close: pdh_close,
            })
        }
    }

    pub fn sample_celsius(&self) -> Option<f32> {
        unsafe {
            if (self.fn_collect)(self.query) != 0 {
                return None;
            }
            let mut val = PdhFmtCountervalue::default();
            if (self.fn_get_val)(self.counter, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut val) != 0 {
                return None;
            }

            let kelvin = if self.is_high_precision {
                val.double_value / 10.0
            } else {
                val.double_value
            };

            let celsius = (kelvin - 273.15) as f32;
            // Reject values < 10°C or > 125°C (uninitialized ACPI zone or invalid sensor)
            if (10.0..125.0).contains(&celsius) {
                Some(celsius)
            } else {
                None
            }
        }
    }
}

impl Drop for PdhThermalReader {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.fn_close)(self.query);
            let _ = FreeLibrary(self.hmodule);
        }
    }
}

pub struct SensorMonitor {
    components: Components,
    pdh_reader: Option<PdhThermalReader>,
}

impl SensorMonitor {
    pub fn new() -> Self {
        let components = Components::new_with_refreshed_list();
        let pdh_reader = PdhThermalReader::try_new();
        Self {
            components,
            pdh_reader,
        }
    }

    /// Attempts to read the CPU temperature.
    /// Priority 1: Native Windows Thermal Zone Performance Counter (pdh.dll)
    /// Priority 2: sysinfo components (if exposed by OEM / ACPI)
    /// Priority 3: None (reports "N/A" gracefully)
    pub fn sample_cpu_temp(&mut self) -> Option<f32> {
        // Priority 1: Windows PDH Thermal Zone
        if let Some(reader) = &self.pdh_reader {
            if let Some(temp) = reader.sample_celsius() {
                return Some(temp);
            }
        }

        // Priority 2: sysinfo components
        self.components.refresh(true);
        for component in &self.components {
            let label = component.label().to_lowercase();
            if label.contains("cpu") || label.contains("core") || label.contains("package") || label.contains("thermal") {
                if let Some(temp) = component.temperature() {
                    // Reject values < 10°C or > 125°C (dummy 0°C or invalid)
                    if temp >= 10.0 && temp <= 125.0 {
                        return Some(temp);
                    }
                }
            }
        }

        // Priority 3: Fallback
        None
    }
}
