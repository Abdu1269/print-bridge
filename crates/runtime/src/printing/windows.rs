use super::{
    submitted_at_rfc3339, sumatra_print_settings,
    windows_buffer::{read_buffer, BufferRead},
    windows_physical_paper_dimensions, PaperInfo, PrintBackend, PrintError, PrintOptions,
    PrintResult, PrintSubmission, PrintTrackingOutcome, PrinterInfo, RawPrintOptions,
};
use serde::Deserialize;
use std::{
    ffi::OsStr,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    ptr,
};
use windows_sys::Win32::Graphics::Printing::{
    ClosePrinter, DocumentPropertiesW, EndDocPrinter, EndPagePrinter, EnumFormsW, GetPrinterW,
    OpenPrinterW, StartDocPrinterW, StartPagePrinter, WritePrinter, DOC_INFO_1W, FORM_INFO_1W,
    PRINTER_HANDLE, PRINTER_INFO_8W, PRINTER_INFO_9W,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER},
    Graphics::Gdi::{
        CreateICW, DeleteDC, GetDeviceCaps, DEVMODEW, DM_FORMNAME, DM_IN_BUFFER, DM_OUT_BUFFER,
        DM_PAPERLENGTH, DM_PAPERSIZE, DM_PAPERWIDTH, LOGPIXELSX, LOGPIXELSY, PHYSICALHEIGHT,
        PHYSICALWIDTH,
    },
};

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Windows printing backend: discovers printers via PowerShell and prints via SumatraPDF.
#[derive(Debug, Clone)]
pub struct WindowsPrintBackend {
    sumatra_path: PathBuf,
}

impl Default for WindowsPrintBackend {
    /// Uses the SumatraPDF executable bundled alongside the application binary.
    fn default() -> Self {
        Self {
            sumatra_path: bundled_sumatra_path(),
        }
    }
}

impl WindowsPrintBackend {
    /// Create backend using explicit SumatraPDF path, primarily for app initialization and testing.
    pub fn new(sumatra_path: impl Into<PathBuf>) -> Self {
        Self {
            sumatra_path: sumatra_path.into(),
        }
    }
}

impl PrintBackend for WindowsPrintBackend {
    /// List Windows printers via PowerShell JSON output.
    fn list_printers(&self) -> PrintResult<Vec<PrinterInfo>> {
        let output = hidden_command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Printer | ForEach-Object { $config = Get-PrintConfiguration -PrinterName $_.Name -ErrorAction SilentlyContinue; [PSCustomObject]@{ Name=$_.Name; Default=$_.Default; PortName=$_.PortName; Type=$_.Type.ToString(); DeviceType=$_.DeviceType.ToString(); DriverName=$_.DriverName; PrintQuality=if ($config) { $config.PrintQuality } else { $null } } } | ConvertTo-Json",
            ])
            .output()
            .map_err(|error| command_error("powershell", error.to_string()))?;

        if !output.status.success() {
            return Err(command_error(
                "powershell",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        parse_printers_json(&String::from_utf8_lossy(&output.stdout))
    }

    /// Read available paper IDs and verified physical millimeter dimensions under current driver orientation.
    fn list_papers(&self, printer_name: &str) -> PrintResult<Vec<PaperInfo>> {
        ensure_printer_exists(self, printer_name)?;
        read_driver_papers(printer_name)
    }

    /// Select driver paper matching task dimensions, sending PDF with explicit copies and content orientation.
    fn print_pdf(&self, path: &Path, options: &PrintOptions) -> PrintResult<PrintSubmission> {
        let papers = self.list_papers(&options.printer_name)?;
        let settings = sumatra_print_settings(options.copies, &options.paper, &papers)?;
        let output = hidden_command(&self.sumatra_path)
            .arg("-silent")
            .arg("-print-to")
            .arg(&options.printer_name)
            .arg("-print-settings")
            .arg(settings)
            .arg(path)
            .output()
            .map_err(|error| command_error("SumatraPDF.exe", error.to_string()))?;

        if output.status.success() {
            Ok(PrintSubmission {
                submitted_at: submitted_at_rfc3339(),
                backend: "windows-sumatra".to_string(),
                system_job_id: None,
                tracking_supported: false,
            })
        } else {
            Err(command_error(
                "SumatraPDF.exe",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }

    /// Submit raw print commands to printer using Windows RAW spooler.
    fn print_raw(&self, data: &[u8], options: &RawPrintOptions) -> PrintResult<PrintSubmission> {
        ensure_printer_exists(self, &options.printer_name)?;
        submit_raw_to_windows_printer(&options.printer_name, data)?;

        Ok(PrintSubmission {
            submitted_at: submitted_at_rfc3339(),
            backend: "windows-raw-spooler".to_string(),
            system_job_id: None,
            tracking_supported: false,
        })
    }

    fn track_submission(
        &self,
        _submission: &PrintSubmission,
        _options: &PrintOptions,
    ) -> PrintTrackingOutcome {
        PrintTrackingOutcome::Unknown {
            message: "Windows SumatraPDF submission did not expose a system print job id"
                .to_string(),
        }
    }
}

/// Safely enumerate forms and verify driver ID physical dimensions under equivalent SumatraPDF settings.
fn read_driver_papers(printer_name: &str) -> PrintResult<Vec<PaperInfo>> {
    let name = wide_null(printer_name);
    let mut printer = PRINTER_HANDLE {
        Value: ptr::null_mut(),
    };
    if unsafe { OpenPrinterW(name.as_ptr(), &mut printer, ptr::null()) } == 0 {
        return Err(command_error("OpenPrinterW", last_os_error()));
    }
    let result = read_validated_papers(printer, &name);
    unsafe { ClosePrinter(printer) };
    result
}

/// Read forms for opened printer, retaining user defaults for paper ID equivalence verification.
fn read_validated_papers(printer: PRINTER_HANDLE, name: &[u16]) -> PrintResult<Vec<PaperInfo>> {
    let default_mode = read_default_devmode(printer, name)?;
    let (forms, count) = read_buffer("EnumFormsW", |buffer| {
        let mut needed = 0;
        let mut count = 0;
        let output = if buffer.is_empty() {
            ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast()
        };
        let ok = unsafe {
            EnumFormsW(
                printer,
                1,
                output,
                (buffer.len() * 8) as u32,
                &mut needed,
                &mut count,
            )
        };
        buffer_read_result(ok, needed, count)
    })?;
    if count > forms.len() * 8 / std::mem::size_of::<FORM_INFO_1W>() {
        return Err(command_error("EnumFormsW", "invalid form count".into()));
    }
    let mut papers = Vec::new();
    for index in 0..count {
        let form = unsafe { &*forms.as_ptr().cast::<FORM_INFO_1W>().add(index) };
        if form.Size.cx <= 0 || form.Size.cy <= 0 || form.pName.is_null() {
            continue;
        }
        let start = form.pName as usize;
        let base = forms.as_ptr() as usize;
        let end = base + forms.len() * 8;
        if start < base || start >= end || !start.is_multiple_of(2) {
            return Err(command_error("EnumFormsW", "invalid form name".into()));
        }
        let raw_name =
            unsafe { std::slice::from_raw_parts(form.pName, ((end - start) / 2).min(32)) };
        let Some(length) = raw_name.iter().position(|value| *value == 0) else {
            continue;
        };
        let mut candidate = default_mode.clone();
        let mode = unsafe { &mut *candidate.as_mut_ptr().cast::<DEVMODEW>() };
        mode.dmFormName.fill(0);
        mode.dmFormName[..length].copy_from_slice(&raw_name[..length]);
        mode.dmFields &= !(DM_PAPERSIZE | DM_PAPERWIDTH | DM_PAPERLENGTH);
        mode.dmFields |= DM_FORMNAME;
        if !validate_devmode(printer, name, &mut candidate) {
            continue;
        }
        let kind = unsafe {
            (*candidate.as_ptr().cast::<DEVMODEW>())
                .Anonymous1
                .Anonymous1
                .dmPaperSize
        };
        if kind <= 0
            || papers
                .iter()
                .any(|paper: &PaperInfo| paper.id == kind.to_string())
        {
            continue;
        }
        // SumatraPDF 3.6.1 only overrides paper ID in default DEVMODE, then delegates verification to driver.
        let mut effective = default_mode.clone();
        let mode = unsafe { &mut *effective.as_mut_ptr().cast::<DEVMODEW>() };
        mode.Anonymous1.Anonymous1.dmPaperSize = kind;
        mode.dmFields |= DM_PAPERSIZE;
        if !validate_devmode(printer, name, &mut effective) {
            continue;
        }
        let Some((width_mm, height_mm)) = physical_paper_size(name, &effective) else {
            continue;
        };
        let form_width = f64::from(form.Size.cx) / 1000.0;
        let form_height = f64::from(form.Size.cy) / 1000.0;
        let Some((width_mm, height_mm)) =
            windows_physical_paper_dimensions(form_width, form_height, width_mm, height_mm)
        else {
            continue;
        };
        papers.push(PaperInfo {
            id: kind.to_string(),
            name: String::from_utf16_lossy(&raw_name[..length]),
            width_mm,
            height_mm,
        });
    }
    Ok(papers)
}

/// Convert sized Win32 query return value into retry or error outcome.
fn buffer_read_result(ok: i32, needed: u32, count: u32) -> BufferRead {
    if ok != 0 {
        BufferRead::Complete(count as usize)
    } else if unsafe { GetLastError() } == ERROR_INSUFFICIENT_BUFFER {
        BufferRead::Resize(needed as usize)
    } else {
        BufferRead::Failed(last_os_error())
    }
}

/// Read user default DEVMODE; falls back to global default configuration if user settings are not configured.
fn read_default_devmode(printer: PRINTER_HANDLE, name: &[u16]) -> PrintResult<Vec<u64>> {
    for level in [9, 8] {
        let (buffer, _) = read_buffer("GetPrinterW", |buffer| {
            let mut needed = 0;
            let output = if buffer.is_empty() {
                ptr::null_mut()
            } else {
                buffer.as_mut_ptr().cast()
            };
            let ok = unsafe {
                GetPrinterW(
                    printer,
                    level,
                    output,
                    (buffer.len() * 8) as u32,
                    &mut needed,
                )
            };
            buffer_read_result(ok, needed, 1)
        })?;
        if buffer.len() * 8 < std::mem::size_of::<PRINTER_INFO_9W>() {
            return Err(command_error(
                "GetPrinterW",
                "invalid printer configuration".into(),
            ));
        }
        let source = unsafe {
            if level == 9 {
                (*buffer.as_ptr().cast::<PRINTER_INFO_9W>()).pDevMode
            } else {
                (*buffer.as_ptr().cast::<PRINTER_INFO_8W>()).pDevMode
            }
        };
        if source.is_null() {
            continue;
        }
        let start = source as usize;
        let base = buffer.as_ptr() as usize;
        let end = base + buffer.len() * 8;
        if start < base || start + std::mem::size_of::<DEVMODEW>() > end {
            return Err(command_error(
                "GetPrinterW",
                "invalid DEVMODE buffer".into(),
            ));
        }
        let public_size = unsafe { usize::from((*source).dmSize) };
        let length = public_size + unsafe { usize::from((*source).dmDriverExtra) };
        if public_size < std::mem::size_of::<DEVMODEW>() || start + length > end {
            return Err(command_error("GetPrinterW", "invalid DEVMODE size".into()));
        }
        // dmSize and dmDriverExtra are WORD: full DEVMODE is at most 2 * u16::MAX bytes.
        // Retaining this upper bound ensures DocumentProperties merge will not overflow even if private data length changes.
        let mut mode = vec![0_u64; (2 * usize::from(u16::MAX)).div_ceil(8)];
        unsafe { ptr::copy_nonoverlapping(source.cast::<u8>(), mode.as_mut_ptr().cast(), length) };
        // As in SumatraPDF NewPrinter, initialize full DEVMODE using current driver default configuration.
        if unsafe {
            DocumentPropertiesW(
                ptr::null_mut(),
                printer,
                name.as_ptr(),
                mode.as_mut_ptr().cast(),
                ptr::null(),
                DM_OUT_BUFFER,
            )
        } != 1
        {
            return Err(command_error("DocumentPropertiesW", last_os_error()));
        }
        return Ok(mode);
    }
    Err(command_error(
        "GetPrinterW",
        "printer has no default DEVMODE".into(),
    ))
}

/// Have driver verify full DEVMODE in memory without persisting to printer defaults.
fn validate_devmode(printer: PRINTER_HANDLE, name: &[u16], mode: &mut [u64]) -> bool {
    let pointer = mode.as_mut_ptr().cast::<DEVMODEW>();
    unsafe {
        DocumentPropertiesW(
            ptr::null_mut(),
            printer,
            name.as_ptr(),
            pointer,
            pointer,
            DM_IN_BUFFER | DM_OUT_BUFFER,
        ) == 1
    }
}

/// Read physical page millimeter dimensions of equivalent print configuration without creating print job.
fn physical_paper_size(name: &[u16], mode: &[u64]) -> Option<(f64, f64)> {
    let dc = unsafe {
        CreateICW(
            ptr::null(),
            name.as_ptr(),
            ptr::null(),
            mode.as_ptr().cast(),
        )
    };
    if dc.is_null() {
        return None;
    }
    let width = unsafe { GetDeviceCaps(dc, PHYSICALWIDTH as i32) };
    let height = unsafe { GetDeviceCaps(dc, PHYSICALHEIGHT as i32) };
    let dpi_x = unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) };
    let dpi_y = unsafe { GetDeviceCaps(dc, LOGPIXELSY as i32) };
    unsafe { DeleteDC(dc) };
    if width <= 0 || height <= 0 || dpi_x <= 0 || dpi_y <= 0 {
        return None;
    }
    Some((
        f64::from(width) * 25.4 / f64::from(dpi_x),
        f64::from(height) * 25.4 / f64::from(dpi_y),
    ))
}

fn submit_raw_to_windows_printer(printer_name: &str, data: &[u8]) -> PrintResult<()> {
    let printer_name_w = wide_null(printer_name);
    let document_name_w = wide_null("PrintBridge Raw Job");
    let data_type_w = wide_null("RAW");
    let mut printer = PRINTER_HANDLE {
        Value: ptr::null_mut(),
    };

    unsafe {
        if OpenPrinterW(
            printer_name_w.as_ptr() as *mut _,
            &mut printer,
            ptr::null_mut(),
        ) == 0
        {
            return Err(command_error("OpenPrinterW", last_os_error()));
        }

        let doc_info = DOC_INFO_1W {
            pDocName: document_name_w.as_ptr() as *mut _,
            pOutputFile: ptr::null_mut(),
            pDatatype: data_type_w.as_ptr() as *mut _,
        };

        if StartDocPrinterW(printer, 1, &doc_info as *const _ as *mut _) == 0 {
            ClosePrinter(printer);
            return Err(command_error("StartDocPrinterW", last_os_error()));
        }

        if StartPagePrinter(printer) == 0 {
            EndDocPrinter(printer);
            ClosePrinter(printer);
            return Err(command_error("StartPagePrinter", last_os_error()));
        }

        let mut written = 0_u32;
        let ok = WritePrinter(
            printer,
            data.as_ptr() as *const _,
            data.len() as u32,
            &mut written,
        );

        EndPagePrinter(printer);
        EndDocPrinter(printer);
        ClosePrinter(printer);

        if ok == 0 || written as usize != data.len() {
            return Err(command_error("WritePrinter", last_os_error()));
        }
    }

    Ok(())
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn last_os_error() -> String {
    std::io::Error::last_os_error().to_string()
}

/// PowerShell ConvertTo-Json may return a single object or an array.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PrinterJson {
    One(PowerShellPrinter),
    Many(Vec<PowerShellPrinter>),
}

/// Raw printer struct returned by PowerShell.
#[derive(Debug, Deserialize)]
struct PowerShellPrinter {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Default", default)]
    default: Option<bool>,
    #[serde(rename = "PortName", default)]
    port_name: Option<String>,
    #[serde(rename = "Type", default)]
    printer_type: Option<String>,
    #[serde(rename = "DeviceType", default)]
    device_type: Option<String>,
    #[serde(rename = "DriverName", default)]
    driver_name: Option<String>,
    #[serde(rename = "PrintQuality", default)]
    print_quality: Option<serde_json::Value>,
}

/// Parse PowerShell printer JSON into platform-independent printer summary.
fn parse_printers_json(value: &str) -> PrintResult<Vec<PrinterInfo>> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }

    let printers = serde_json::from_str::<PrinterJson>(value).map_err(|error| {
        command_error(
            "powershell",
            format!("failed to parse printer json: {error}"),
        )
    })?;

    Ok(match printers {
        PrinterJson::One(printer) => vec![printer.into()],
        PrinterJson::Many(printers) => printers.into_iter().map(Into::into).collect(),
    })
}

impl From<PowerShellPrinter> for PrinterInfo {
    /// Convert PowerShell field names into shared printer types.
    fn from(value: PowerShellPrinter) -> Self {
        let dpi = value
            .print_quality
            .as_ref()
            .and_then(parse_print_quality_dpi);
        let port = value.port_name.clone();
        let (is_local, is_network, is_virtual) = classify_windows_printer(
            value.name.as_str(),
            value.printer_type.as_deref(),
            value.device_type.as_deref(),
            value.driver_name.as_deref(),
            port.as_deref(),
        );

        Self {
            name: value.name,
            is_default: value.default.unwrap_or(false),
            dpi,
            port,
            is_local,
            is_network,
            is_virtual,
        }
    }
}

fn parse_print_quality_dpi(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::Number(number) => {
            number.as_u64().and_then(|value| value.try_into().ok())
        }
        serde_json::Value::String(value) => parse_dpi_text(value),
        _ => None,
    }
}

fn parse_dpi_text(value: &str) -> Option<u32> {
    let lower = value.to_ascii_lowercase();
    let dpi_index = lower.find("dpi").unwrap_or(lower.len());
    lower[..dpi_index]
        .split(|character: char| !character.is_ascii_digit())
        .filter_map(|part| part.parse::<u32>().ok())
        .max()
}

fn classify_windows_printer(
    name: &str,
    printer_type: Option<&str>,
    device_type: Option<&str>,
    driver_name: Option<&str>,
    port: Option<&str>,
) -> (Option<bool>, Option<bool>, Option<bool>) {
    let text = [Some(name), printer_type, device_type, driver_name, port]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let port = port.unwrap_or_default().to_ascii_lowercase();

    let is_virtual = text.contains("pdf")
        || text.contains("xps")
        || text.contains("onenote")
        || text.contains("fax")
        || text.contains("document writer");
    let is_network = port.starts_with("\\\\")
        || port.starts_with("ip_")
        || port.starts_with("tcp")
        || port.starts_with("http")
        || port.starts_with("wsd")
        || text.contains("connection")
        || text.contains("network");
    let is_local = port.starts_with("usb")
        || port.starts_with("lpt")
        || port.starts_with("com")
        || text.contains("local");

    if is_virtual {
        (Some(false), Some(false), Some(true))
    } else if is_network {
        (Some(false), Some(true), Some(false))
    } else if is_local {
        (Some(true), Some(false), Some(false))
    } else {
        (None, None, None)
    }
}

/// Ensures a printer exists before paper lookup or printing.
fn ensure_printer_exists(backend: &WindowsPrintBackend, printer_name: &str) -> PrintResult<()> {
    if backend
        .list_printers()?
        .iter()
        .any(|printer| printer.name == printer_name)
    {
        Ok(())
    } else {
        Err(PrintError::PrinterNotFound(printer_name.to_string()))
    }
}

/// Create Windows child process without spawning detached console window.
fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Locate bundled SumatraPDF next to current executable, with PATH fallback.
fn bundled_sumatra_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("SumatraPDF.exe")))
        .unwrap_or_else(|| PathBuf::from("SumatraPDF.exe"))
}

/// Construct PrintError for failed platform command.
fn command_error(command: &str, message: String) -> PrintError {
    PrintError::CommandFailed {
        command: command.to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_printers_json;

    #[test]
    fn parse_printers_json_accepts_null_default_flag() {
        let printers = parse_printers_json(
            r#"[{"Name":"Printer A","Default":null,"PortName":"USB001","Type":"Local","DeviceType":"Print","DriverName":"Driver"}]"#,
        )
        .unwrap();

        assert_eq!(printers.len(), 1);
        assert!(!printers[0].is_default);
        assert_eq!(printers[0].name, "Printer A");
    }
}
