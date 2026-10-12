//! Windows network lookups for the tunnel.

use std::net::Ipv4Addr;

/// Index of the network adapter that holds `ip` (the address the VPN gave
/// us), or None when no adapter has it (the tunnel is gone).
pub fn interface_of(ip: Ipv4Addr) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetUnicastIpAddressTable, MIB_UNICASTIPADDRESS_TABLE,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    let mut table: *mut MIB_UNICASTIPADDRESS_TABLE = std::ptr::null_mut();
    if unsafe { GetUnicastIpAddressTable(AF_INET, &mut table) } != 0 || table.is_null() {
        return None;
    }
    let found = unsafe {
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        rows.iter()
            .find(|r| {
                r.Address.si_family == AF_INET
                    && Ipv4Addr::from(u32::from_be(r.Address.Ipv4.sin_addr.S_un.S_addr)) == ip
            })
            .map(|r| r.InterfaceIndex)
    };
    unsafe { FreeMibTable(table as _) };
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_has_an_adapter() {
        assert!(interface_of(Ipv4Addr::LOCALHOST).is_some());
        assert_eq!(interface_of(Ipv4Addr::new(203, 0, 113, 77)), None);
    }
}
