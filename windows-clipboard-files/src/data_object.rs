use parking_lot::Mutex;
use windows::{
    core::HRESULT,
    Win32::{
        Foundation::{BOOL, E_FAIL, E_NOTIMPL, OLE_E_ADVISENOTSUPPORTED, S_FALSE, S_OK},
        System::Com::{
            IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
            ISequentialStream, ISequentialStream_Impl, IStream, IStream_Impl, DATADIR_GET,
            FORMATETC, LOCKTYPE, STATFLAG, STATSTG, STGC, STGMEDIUM, STREAM_SEEK, STGTY_STREAM, STGM, STGM_READ,
        },
    },
};

#[windows::core::implement(IDataObject)]
struct ClipboardDataObject {}

#[allow(non_snake_case)]
impl IDataObject_Impl for ClipboardDataObject {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let mut ret = STGMEDIUM::default();
        self.GetDataHere(pformatetcin, &mut ret)?;
        Ok(ret)
    }

    fn GetDataHere(
        &self,
        pformatetc: *const FORMATETC,
        pmedium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        todo!()
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        todo!()
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }

        todo!()
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: ::core::option::Option<&IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[windows::core::implement(ISequentialStream, IStream)]
pub struct ReadWrapper {
    inner: Mutex<Box<dyn std::io::Read>>,
    len: u64,
}

#[allow(non_snake_case)]
impl ISequentialStream_Impl for ReadWrapper {
    fn Read(&self, pv: *mut ::core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        let mut inner = self.inner.lock();

        let mut dest_buffer = unsafe { std::slice::from_raw_parts_mut(pv as *mut u8, cb as usize) };
        let mut n = 0;

        loop {
            match inner.read(dest_buffer) {
                Ok(0) => {
                    break;
                }
                Ok(filled_size) => {
                    n += filled_size as u32;

                    if cb == n {
                        break;
                    }

                    dest_buffer = &mut dest_buffer[filled_size..];
                }
                Err(_e) => {
                    return E_FAIL;
                }
            }
        }

        assert!(n <= cb);

        if !pcbread.is_null() {
            unsafe {
                *pcbread = n;
            }
        }

        if cb == n {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Write(&self, _pv: *const ::core::ffi::c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        E_NOTIMPL
    }
}

#[allow(non_snake_case)]
impl IStream_Impl for ReadWrapper {
    fn Seek(
        &self,
        _dlibmove: i64,
        _dworigin: STREAM_SEEK,
        _plibnewposition: *mut u64,
    ) -> ::windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn CopyTo(
        &self,
        _pstm: Option<&IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn LockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: &LOCKTYPE,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn UnlockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: u32,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows::core::Result<()> {
        let stat = unsafe { pstatstg.as_mut().unwrap() };
        stat.r#type = STGTY_STREAM.0 as u32;
        stat.cbSize = self.len;
        stat.grfMode = STGM_READ;
        
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}
