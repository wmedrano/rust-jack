// The MIT License (MIT)
//
// Copyright (c) 2017 Will Medrano (will.s.medrano@gmail.com)
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use std::ffi;

use jack_sys as j;
use libc;

use jack_enums::*;
use port::port_flags::PortFlags;
use jack_utils::collect_strs;
use port::{Port, PortSpec, UnownedPort};
use port;
use primitive_types as pt;

/// Internal cycle timing information.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CycleTimes {
    pub current_frames: pt::JackFrames,
    pub current_usecs: pt::JackTime,
    pub next_usecs: pt::JackTime,
    pub period_usecs: libc::c_float,
}

/// `ProcessScope` provides information on the client and frame time information within a process
/// callback.
#[derive(Debug)]
pub struct ProcessScope {
    client_ptr: *mut j::jack_client_t,

    // Used to allow safe access to IO port buffers
    n_frames: pt::JackFrames,
}

impl ProcessScope {
    /// The number of frames in the current process cycle.
    #[inline(always)]
    pub fn n_frames(&self) -> pt::JackFrames {
        self.n_frames
    }

    /// The precise time at the start of the current process cycle. This function may only be used
    /// from the process callback, and can be used to interpret timestamps generated by
    /// `self.frame_time()` in other threads, with respect to the current process cycle.
    pub fn last_frame_time(&self) -> pt::JackFrames {
        unsafe { j::jack_last_frame_time(self.client_ptr()) }
    }

    /// The estimated time in frames that has passed since the JACK server began the current process
    /// cycle.
    pub fn frames_since_cycle_start(&self) -> pt::JackFrames {
        unsafe { j::jack_frames_since_cycle_start(self.client_ptr()) }
    }

    /// Provides the internal cycle timing information as used by most of the other time related
    /// functions. This allows the caller to map between frame counts and microseconds with full
    /// precision (i.e. without rounding frame times to integers), and also provides e.g. the
    /// microseconds time of the start of the current cycle directly (it has to be computed
    /// otherwise).
    ///
    /// `Err(JackErr::TimeError)` is returned on failure.
    ///
    /// TODO
    /// - implement, weakly exported in JACK, so it is not defined to always be available.
    pub fn cycle_times(&self) -> Result<CycleTimes, JackErr> {
        unimplemented!();
        // let mut current_frames: pt::JackFrames = 0;
        // let mut current_usecs: pt::JackTime = 0;
        // let mut next_usecs: pt::JackTime = 0;
        // let mut period_usecs: libc::c_float = 0.0;
        // let res = unsafe {
        //     j::jack_get_cycle_times(self.client_ptr(),
        //                             &mut current_frames,
        //                             &mut current_usecs,
        //                             &mut next_usecs,
        //                             &mut period_usecs)
        // };
        // match res {
        //     0 => {
        //         Ok(CycleTimes {
        //             current_frames: current_frames,
        //             current_usecs: current_usecs,
        //             next_usecs: next_usecs,
        //             period_usecs: period_usecs,
        //         })
        //     }
        //     _ => Err(JackErr::TimeError),
        // }
    }

    /// Expose the `client_ptr` for low level purposes.
    ///
    /// This is mostly for use within the jack crate itself.
    #[inline(always)]
    pub fn client_ptr(&self) -> *mut j::jack_client_t {
        self.client_ptr
    }

    /// Create a `ProcessScope` for the client with the given pointer and the specified amount of
    /// frames.
    ///
    /// This is mostly for use within the jack crate itself.
    pub unsafe fn from_raw(n_frames: pt::JackFrames, client_ptr: *mut j::jack_client_t) -> Self {
        ProcessScope {
            n_frames: n_frames,
            client_ptr: client_ptr,
        }
    }
}


/// Similar to a `Client`, but usually exposed only through reference.
///
/// On a technical level, the difference between `WeakClient` and `Client` is that `WeakClient` does
/// not close on drop.
#[derive(Debug)]
#[repr(C)]
pub struct WeakClient(*mut j::jack_client_t);

impl WeakClient {
    /// Construct a `WeakClient`.
    ///
    /// This is mostly for use within the jack crate itself.
    pub unsafe fn from_raw(client_ptr: *mut j::jack_client_t) -> Self {
        WeakClient(client_ptr)
    }
}

unsafe impl JackClient for WeakClient {
    fn as_ptr(&self) -> *mut j::jack_client_t {
        self.0
    }
}

/// Common JACK client functionality that can be accessed for both inactive and active clients.
pub unsafe trait JackClient: Sized {
    /// The sample rate of the JACK system, as set by the user when jackd was started.
    fn sample_rate(&self) -> usize {
        let srate = unsafe { j::jack_get_sample_rate(self.as_ptr()) };
        srate as usize
    }

    /// The current CPU load estimated by JACK.
    ///
    /// This is a running average of the time it takes to execute a full process cycle for all
    /// clients as a percentage of the real time available per cycle determined by the buffer size
    /// and sample rate.
    fn cpu_load(&self) -> libc::c_float {
        let load = unsafe { j::jack_cpu_load(self.as_ptr()) };
        load
    }


    /// Get the name of the current client. This may differ from the name requested by
    /// `Client::open` as JACK will may rename a client if necessary (ie: name collision, name too
    /// long). The name will only the be different than the one passed to `Client::open` if the
    /// `ClientStatus` was `NAME_NOT_UNIQUE`.
    fn name<'a>(&'a self) -> &'a str {
        unsafe {
            let ptr = j::jack_get_client_name(self.as_ptr());
            let cstr = ffi::CStr::from_ptr(ptr);
            cstr.to_str().unwrap()
        }
    }

    /// The current maximum size that will every be passed to the process callback.
    fn buffer_size(&self) -> pt::JackFrames {
        unsafe { j::jack_get_buffer_size(self.as_ptr()) }
    }

    /// Change the buffer size passed to the process callback.
    ///
    /// This operation stops the JACK engine process cycle, then calls all registered buffer size
    /// callback functions before restarting the process cycle. This will cause a gap in the audio
    /// flow, so it should only be done at appropriate stopping points.
    fn set_buffer_size(&self, n_frames: pt::JackFrames) -> Result<(), JackErr> {
        let res = unsafe { j::jack_set_buffer_size(self.as_ptr(), n_frames) };
        match res {
            0 => Ok(()),
            _ => Err(JackErr::SetBufferSizeError),
        }
    }
    // TODO implement
    // /// Get the uuid of the current client.
    // fn uuid<'a>(&'a self) -> &'a str {
    //     self.uuid_by_name(self.name()).unwrap_or("")
    // }

    // TODO implement
    // // Get the name of the client with the UUID specified by `uuid`. If the
    // // client is found then `Some(name)` is returned, if not, then `None` is
    // // returned.
    // // fn name_by_uuid<'a>(&'a self, uuid: &str) -> Option<&'a str> {
    //     unsafe {
    //         let uuid = ffi::CString::new(uuid).unwrap();
    //         let name_ptr = j::jack_get_client_name_by_uuid(self.as_ptr(), uuid.as_ptr());
    //         if name_ptr.is_null() {
    //             None
    //         } else {
    //             Some(ffi::CStr::from_ptr(name_ptr).to_str().unwrap())
    //         }
    //     }
    // }

    // TODO implement
    // /// Get the uuid of the client with the name specified by `name`. If the
    // /// client is found then `Some(uuid)` is returned, if not, then `None` is
    // /// returned.
    // fn uuid_by_name<'a>(&'a self, name: &str) -> Option<&'a str> {
    //     unsafe {
    //         let name = ffi::CString::new(name).unwrap();
    //         let uuid_ptr = j::jack_get_client_name_by_uuid(self.as_ptr(), name.as_ptr());
    //         if uuid_ptr.is_null() {
    //             None
    //         } else {
    //             Some(ffi::CStr::from_ptr(uuid_ptr).to_str().unwrap())
    //         }
    //     }
    // }

    /// Returns a vector of port names that match the specified arguments
    ///
    /// `port_name_pattern` - A regular expression used to select ports by
    /// name. If `None` or zero lengthed, no selection based on name will be
    /// carried out.
    ///
    /// `type_name_pattern` - A regular expression used to select ports by type. If `None` or zero
    /// lengthed, no selection based on type will be carried out. The port type is the same one
    /// returned by `PortSpec::jack_port_type()`. For example, `AudioInSpec` and `AudioOutSpec` are
    /// both of type `"32 bit float mono audio"`.
    ///
    /// `flags` - A value used to select ports by their flags. Use
    /// `PortFlags::empty()` for no flag selection.
    fn ports(&self,
             port_name_pattern: Option<&str>,
             type_name_pattern: Option<&str>,
             flags: PortFlags)
             -> Vec<String> {
        let pnp = ffi::CString::new(port_name_pattern.unwrap_or("")).unwrap();
        let tnp = ffi::CString::new(type_name_pattern.unwrap_or("")).unwrap();
        let flags = flags.bits() as libc::c_ulong;
        unsafe {
            let ports = j::jack_get_ports(self.as_ptr(), pnp.as_ptr(), tnp.as_ptr(), flags);
            collect_strs(ports)
        }
    }

    /// Create a new port for the client. This is an object used for moving data
    /// of any type in or out of the client. Ports may be connected in various
    /// ways.
    ///
    /// Each port has a short name. The port's full name contains the name of
    /// the client concatenated with a colon (:) followed by its short
    /// name. `Port::name_size()` is the maximum length of the full
    /// name. Exceeding that will cause the port registration to fail and return
    /// `Err(())`.
    ///
    /// The `port_name` must be unique among all ports owned by this client. If
    /// the name is not unique, the registration will fail.
    fn register_port<PS: PortSpec>(&self,
                                   port_name: &str,
                                   port_spec: PS)
                                   -> Result<Port<PS>, JackErr> {
        let port_name_c = ffi::CString::new(port_name).unwrap();
        let port_type_c = ffi::CString::new(port_spec.jack_port_type()).unwrap();
        let port_flags = port_spec.jack_flags().bits();
        let buffer_size = port_spec.jack_buffer_size();
        let pp = unsafe {
            j::jack_port_register(self.as_ptr(),
                                  port_name_c.as_ptr(),
                                  port_type_c.as_ptr(),
                                  port_flags as libc::c_ulong,
                                  buffer_size)
        };
        if pp.is_null() {
            Err(JackErr::PortRegistrationError(port_name.to_string()))
        } else {
            Ok(unsafe { Port::from_raw(port_spec, self.as_ptr(), pp) })
        }
    }



    // Get a `Port` by its port id.
    fn port_by_id(&self, port_id: pt::JackPortId) -> Option<UnownedPort> {
        let pp = unsafe { j::jack_port_by_id(self.as_ptr(), port_id) };
        if pp.is_null() {
            None
        } else {
            Some(unsafe { Port::from_raw(port::Unowned {}, self.as_ptr(), pp) })
        }
    }

    /// Get a `Port` by its port name.
    fn port_by_name(&self, port_name: &str) -> Option<UnownedPort> {
        let port_name = ffi::CString::new(port_name).unwrap();
        let pp = unsafe { j::jack_port_by_name(self.as_ptr(), port_name.as_ptr()) };
        if pp.is_null() {
            None
        } else {
            Some(unsafe { Port::from_raw(port::Unowned {}, self.as_ptr(), pp) })
        }
    }

    /// The estimated time in frames that has passed since the JACK server began the current process
    /// cycle.
    fn frames_since_cycle_start(&self) -> pt::JackFrames {
        unsafe { j::jack_frames_since_cycle_start(self.as_ptr()) }
    }

    /// The estimated current time in frames. This function is intended for use in other threads
    /// (not the process callback). The return value can be compared with the value of
    /// `last_frame_time` to relate time in other threads to JACK time. To obtain better time
    /// information from within the process callback, see `ProcessScope`.
    ///
    /// # TODO
    /// - test
    fn frame_time(&self) -> pt::JackFrames {
        unsafe { j::jack_frame_time(self.as_ptr()) }
    }

    /// The estimated time in microseconds of the specified frame time
    ///
    /// # TODO
    /// - Improve test
    fn frames_to_time(&self, n_frames: pt::JackFrames) -> pt::JackTime {
        unsafe { j::jack_frames_to_time(self.as_ptr(), n_frames) }
    }

    /// The estimated time in frames for the specified system time.
    ///
    /// # TODO
    /// - Improve test
    fn time_to_frames(&self, t: pt::JackTime) -> pt::JackFrames {
        unsafe { j::jack_time_to_frames(self.as_ptr(), t) }
    }

    /// Returns `true` if the port `port` belongs to this client.
    fn is_mine<PS: PortSpec>(&self, port: &Port<PS>) -> bool {
        match unsafe { j::jack_port_is_mine(self.as_ptr(), port.as_ptr()) } {
            1 => true,
            _ => false,
        }
    }

    /// Toggle input monitoring for the port with name `port_name`.
    ///
    /// `Err(JackErr::PortMonitorError)` is returned on failure.
    ///
    /// Only works if the port has the `CAN_MONITOR` flag, or else nothing
    /// happens.
    fn request_monitor_by_name(&self,
                               port_name: &str,
                               enable_monitor: bool)
                               -> Result<(), JackErr> {
        let port_name_cstr = ffi::CString::new(port_name).unwrap();
        let res = unsafe {
            j::jack_port_request_monitor_by_name(self.as_ptr(),
                                                 port_name_cstr.as_ptr(),
                                                 if enable_monitor { 1 } else { 0 })
        };
        match res {
            0 => Ok(()),
            _ => Err(JackErr::PortMonitorError),
        }
    }


    // TODO implement
    // /// Start/Stop JACK's "freewheel" mode.
    // ///
    // /// When in "freewheel" mode, JACK no longer waits for any external event to
    // /// begin the start of the next process cycle. As a result, freewheel mode
    // /// causes "faster than real-time" execution of a JACK graph. If possessed,
    // /// real-time scheduling is dropped when entering freewheel mode, and if
    // /// appropriate it is reacquired when stopping.
    // ///
    // /// IMPORTANT: on systems using capabilities to provide real-time scheduling
    // /// (i.e. Linux Kernel 2.4), if enabling freewheel, this function must be
    // /// called from the thread that originally called `self.activate()`. This
    // /// restriction does not apply to other systems (e.g. Linux Kernel 2.6 or OS
    // /// X).
    // pub fn set_freewheel(&self, enable: bool) -> Result<(), JackErr> {
    //     let onoff = match enable {
    //         true => 0,
    //         false => 1,
    //     };
    //     match unsafe { j::jack_set_freewheel(self.as_ptr(), onoff) } {
    //         0 => Ok(()),
    //         _ => Err(JackErr::FreewheelError),
    //     }
    // }



    /// Establish a connection between two ports by their full name.
    ///
    /// When a connection exists, data written to the source port will be
    /// available to be read at the destination port.
    ///
    /// On failure, either a `PortAlreadyConnected` or `PortConnectionError` is returned.
    ///
    /// # Preconditions
    /// 1. The port types must be identical
    /// 2. The port flags of the `source_port` must include `IS_OUTPUT`
    /// 3. The port flags of the `destination_port` must include `IS_INPUT`.
    /// 4. Both ports must be owned by active clients.
    fn connect_ports_by_name(&self,
                             source_port: &str,
                             destination_port: &str)
                             -> Result<(), JackErr> {
        let source_cstr = ffi::CString::new(source_port).unwrap();
        let destination_cstr = ffi::CString::new(destination_port).unwrap();

        let res = unsafe {
            j::jack_connect(self.as_ptr(),
                            source_cstr.as_ptr(),
                            destination_cstr.as_ptr())
        };
        match res {
            0 => Ok(()),
            ::libc::EEXIST => {
                Err(JackErr::PortAlreadyConnected(source_port.to_string(),
                                                  destination_port.to_string()))
            }
            _ => {
                Err(JackErr::PortConnectionError(source_port.to_string(),
                                                 destination_port.to_string()))
            }
        }
    }

    /// Establish a connection between two ports.
    ///
    /// When a connection exists, data written to the source port will be
    /// available to be read at the destination port.
    ///
    /// On failure, either a `PortAlreadyConnected` or `PortConnectionError` is returned.
    ///
    /// # Preconditions
    /// 1. The port types must be identical
    /// 2. The port flags of the `source_port` must include `IS_OUTPUT`
    /// 3. The port flags of the `destination_port` must include `IS_INPUT`.
    /// 4. Both ports must be owned by active clients.
    fn connect_ports<A: PortSpec, B: PortSpec>(&self,
                                               source_port: &Port<A>,
                                               destination_port: &Port<B>)
                                               -> Result<(), JackErr> {
        self.connect_ports_by_name(source_port.name(), destination_port.name())
    }

    /// Remove a connection between two ports.
    fn disconnect_ports<A: PortSpec, B: PortSpec>(&self,
                                                  source: &Port<A>,
                                                  destination: &Port<B>)
                                                  -> Result<(), JackErr> {
        self.disconnect_ports_by_name(source.name(), destination.name())
    }

    /// Remove a connection between two ports.
    fn disconnect_ports_by_name(&self,
                                source_port: &str,
                                destination_port: &str)
                                -> Result<(), JackErr> {
        let source_port = ffi::CString::new(source_port).unwrap();
        let destination_port = ffi::CString::new(destination_port).unwrap();
        let res = unsafe {
            j::jack_disconnect(self.as_ptr(),
                               source_port.as_ptr(),
                               destination_port.as_ptr())
        };
        match res {
            0 => Ok(()),
            _ => Err(JackErr::PortDisconnectionError),
        }
    }

    /// The buffer size of a port type
    ///
    /// # Unsafe
    ///
    /// * This function may only be called in a buffer size callback.
    unsafe fn type_buffer_size(&self, port_type: &str) -> usize {
        let port_type = ffi::CString::new(port_type).unwrap();
        let n = j::jack_port_type_get_buffer_size(self.as_ptr(), port_type.as_ptr());
        n
    }

    /// Expose the underlying ffi pointer.
    ///
    /// This is mostly for use within the jack crate itself.
    #[inline(always)]
    fn as_ptr(&self) -> *mut j::jack_client_t;
}