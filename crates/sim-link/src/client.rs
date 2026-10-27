use std::collections::HashSet;
use std::ffi::{CString, c_char};
use std::mem::size_of;
use std::{fmt, ptr, slice};

use crate::facility::Requests;
use crate::ffi::{self, HResult, InitPosition};

#[derive(Debug)]
pub enum Error {
    /// A SimConnect call returned a failing HRESULT. `SimConnect_Open` fails
    /// this way when the simulator is not running.
    Call { func: &'static str, hr: HResult },
    /// Names passed to SimConnect cannot contain NUL bytes.
    InvalidName(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call { func, hr } => write!(f, "{func} failed with HRESULT {:#010x}", *hr as u32),
            Self::InvalidName(name) => write!(f, "name contains NUL: {name:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// One message from the SimConnect queue, copied out of SimConnect's buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum Recv {
    Open {
        simconnect_version: [u32; 2],
        simconnect_build: [u32; 2],
    },
    Quit,
    /// Asynchronous failure of an earlier call, identified by its send id.
    Exception {
        code: u32,
        send_id: u32,
        index: u32,
    },
    Event {
        id: u32,
        data: u32,
    },
    EventFilename {
        id: u32,
        file: String,
    },
    /// MSFS 2024 flow event; `event` is a `SIMCONNECT_FLOW_EVENT` value.
    Flow {
        event: u32,
        flt_path: String,
    },
    /// Values of a FLOAT64-only data definition, in definition order.
    Data {
        request_id: u32,
        values: Vec<f64>,
    },
    /// Values of a text definition (see `SimConnect::add_text`), in
    /// definition order.
    Text {
        request_id: u32,
        values: Vec<String>,
    },
    /// A facility-database reply.
    Facility(Facility),
    /// A Comm Bus event subscribed with `subscribe_comm_bus`: its id and
    /// payload, up to the first NUL.
    CommBus {
        event_id: u32,
        data: String,
    },
    /// A message we do not decode (or a truncated one).
    Other {
        id: u32,
    },
}

/// One airport of the simulator's reality-bubble list.
#[derive(Debug, Clone, PartialEq)]
pub struct AirportListEntry {
    pub ident: String,
    pub region: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub elevation_m: f64,
}

/// One node of a facility-data reply: the fields a definition opened for
/// one structure (airport, runway, threshold...), still undecoded.
#[derive(Debug, Clone, PartialEq)]
pub struct FacilityData {
    /// The request id we chose; every node of one request carries it.
    pub request_id: u32,
    /// Node id assigned by SimConnect; children name it as their parent.
    pub unique_id: u32,
    pub parent_id: u32,
    /// `SIMCONNECT_FACILITY_DATA_TYPE` of this node.
    pub data_type: u32,
    pub is_list_item: u32,
    pub item_index: u32,
    pub list_size: u32,
    /// The node's fields packed in definition order.
    pub payload: Vec<u8>,
}

/// Facility-database replies.
#[derive(Debug, Clone, PartialEq)]
pub enum Facility {
    /// One chunk of an airport list; the list is complete when
    /// `entry_number + 1 == out_of`.
    AirportList {
        request_id: u32,
        entry_number: u32,
        out_of: u32,
        airports: Vec<AirportListEntry>,
    },
    Data(FacilityData),
    /// All nodes of a facility-data request have been sent.
    End {
        request_id: u32,
    },
}

/// An open connection to the simulator. Closed on drop. Not `Send`: the
/// handle belongs to the thread that opened it.
pub struct SimConnect {
    handle: ffi::Handle,
    /// Definitions built with `add_text`, whose data decodes as strings.
    text_defines: HashSet<u32>,
    /// Definitions already holding the "Initial Position" datum.
    init_defines: HashSet<u32>,
}

impl SimConnect {
    pub fn open(app_name: &str) -> Result<Self, Error> {
        let name = c_string(app_name)?;
        let mut handle = ptr::null_mut();
        // SAFETY: valid out-pointer and NUL-terminated name; no window or event handle.
        let hr = unsafe {
            ffi::open(
                &mut handle,
                name.as_ptr(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ffi::OPEN_CONFIGINDEX_LOCAL,
            )
        };
        check("SimConnect_Open", hr)?;
        Ok(Self {
            handle,
            text_defines: HashSet::new(),
            init_defines: HashSet::new(),
        })
    }

    /// Appends a SimVar to a data definition as FLOAT64 in `unit`. Returns the
    /// send id, which an `Exception` carries if the name or unit is rejected.
    pub fn add_f64(&mut self, define_id: u32, simvar: &str, unit: &str) -> Result<u32, Error> {
        self.add_to_definition(define_id, simvar, Some(unit), ffi::DATATYPE_FLOAT64)
    }

    /// Appends a string SimVar (`TITLE`, `ATC MODEL`...) to a text
    /// definition as STRING256, the widest fixed size, so no aircraft title
    /// is cut short. A definition is either all FLOAT64 or all text: its
    /// data is decoded by which kind it was built as. Returns the send id.
    pub fn add_text(&mut self, define_id: u32, simvar: &str) -> Result<u32, Error> {
        self.text_defines.insert(define_id);
        // Strings take no unit.
        self.add_to_definition(define_id, simvar, Some(""), ffi::DATATYPE_STRING256)
    }

    /// Puts the user aircraft at `pos`, as a flight start does: the
    /// "Initial Position" datum of definition `define_id` (built on first
    /// use, so give it an id of its own) set on the user aircraft. Returns
    /// the send id, which an `Exception` carries if the sim rejects it.
    pub fn set_init_position(&mut self, define_id: u32, pos: &InitPosition) -> Result<u32, Error> {
        if !self.init_defines.contains(&define_id) {
            // No unit: the SDK passes NULL for structured datums.
            let datatype = ffi::DATATYPE_INITPOSITION;
            self.add_to_definition(define_id, "Initial Position", None, datatype)?;
            self.init_defines.insert(define_id);
        }
        // SAFETY: open handle; `pos` is one `InitPosition`, the size passed,
        // which SimConnect only reads, during the call.
        let hr = unsafe {
            ffi::set_data_on_sim_object(
                self.handle,
                define_id,
                ffi::OBJECT_ID_USER_AIRCRAFT,
                ffi::DATA_SET_FLAG_DEFAULT,
                0,
                size_of::<InitPosition>() as u32,
                ptr::from_ref(pos).cast_mut().cast(),
            )
        };
        check("SimConnect_SetDataOnSimObject", hr)?;
        self.last_sent_packet_id()
    }

    /// Names key event `name` (`GEAR_DOWN`, `FLAPS_SET`...) as client event
    /// `event_id` for `transmit_event`. Keep ids apart from those of
    /// `subscribe_system_event`: they share one space. Returns the send id,
    /// which an `Exception` carries if the name is unknown.
    pub fn map_event(&mut self, event_id: u32, name: &str) -> Result<u32, Error> {
        let name = c_string(name)?;
        // SAFETY: open handle and a NUL-terminated name that outlives the call.
        let hr =
            unsafe { ffi::map_client_event_to_sim_event(self.handle, event_id, name.as_ptr()) };
        check("SimConnect_MapClientEventToSimEvent", hr)?;
        self.last_sent_packet_id()
    }

    /// Sends a mapped key event with `data` to the user aircraft at the
    /// highest priority, as a cockpit control would. Returns the send id.
    pub fn transmit_event(&mut self, event_id: u32, data: u32) -> Result<u32, Error> {
        // SAFETY: open handle; scalar arguments only.
        let hr = unsafe {
            ffi::transmit_client_event(
                self.handle,
                ffi::OBJECT_ID_USER_AIRCRAFT,
                event_id,
                data,
                ffi::GROUP_PRIORITY_HIGHEST,
                ffi::EVENT_FLAG_GROUPID_IS_PRIORITY,
            )
        };
        check("SimConnect_TransmitClientEvent", hr)?;
        self.last_sent_packet_id()
    }

    /// Streams a definition for the user aircraft once per simulation frame.
    pub fn request_every_sim_frame(
        &mut self,
        request_id: u32,
        define_id: u32,
    ) -> Result<(), Error> {
        self.request(request_id, define_id, ffi::PERIOD_SIM_FRAME, 0)
    }

    /// Sends a definition for the user aircraft now, then whenever one of its
    /// values changes, checked once per second.
    pub fn request_on_change(&mut self, request_id: u32, define_id: u32) -> Result<(), Error> {
        self.request(
            request_id,
            define_id,
            ffi::PERIOD_SECOND,
            ffi::DATA_REQUEST_FLAG_CHANGED,
        )
    }

    fn add_to_definition(
        &mut self,
        define_id: u32,
        simvar: &str,
        unit: Option<&str>,
        datatype: i32,
    ) -> Result<u32, Error> {
        let (simvar, unit) = (c_string(simvar)?, unit.map(c_string).transpose()?);
        // SAFETY: open handle and NUL-terminated strings (or no unit) that
        // outlive the call.
        let hr = unsafe {
            ffi::add_to_data_definition(
                self.handle,
                define_id,
                simvar.as_ptr(),
                unit.as_ref().map_or(ptr::null(), |u| u.as_ptr()),
                datatype,
                0.0,
                ffi::UNUSED,
            )
        };
        check("SimConnect_AddToDataDefinition", hr)?;
        self.last_sent_packet_id()
    }

    fn request(
        &mut self,
        request_id: u32,
        define_id: u32,
        period: i32,
        flags: u32,
    ) -> Result<(), Error> {
        // SAFETY: open handle; scalar arguments only.
        let hr = unsafe {
            ffi::request_data_on_sim_object(
                self.handle,
                request_id,
                define_id,
                ffi::OBJECT_ID_USER_AIRCRAFT,
                period,
                flags,
                0,
                0,
                0,
            )
        };
        check("SimConnect_RequestDataOnSimObject", hr)
    }

    pub fn subscribe_system_event(&mut self, event_id: u32, name: &str) -> Result<(), Error> {
        let name = c_string(name)?;
        // SAFETY: open handle and a NUL-terminated name that outlives the call.
        let hr = unsafe { ffi::subscribe_to_system_event(self.handle, event_id, name.as_ptr()) };
        check("SimConnect_SubscribeToSystemEvent", hr)
    }

    pub fn subscribe_flow_events(&mut self) -> Result<(), Error> {
        // SAFETY: open handle.
        let hr = unsafe { ffi::subscribe_to_flow_event(self.handle) };
        check("SimConnect_SubscribeToFlowEvent", hr)
    }

    /// Next queued message, or `None` when the queue is empty.
    pub fn dispatch(&mut self) -> Option<Recv> {
        let mut data = ptr::null_mut();
        let mut len = 0;
        // SAFETY: valid out-pointers on an open handle.
        let hr = unsafe { ffi::get_next_dispatch(self.handle, &mut data, &mut len) };
        if hr < 0 || data.is_null() {
            return None;
        }
        // SAFETY: SimConnect hands out `len` readable bytes that stay valid
        // until the next dispatch call, which the `&mut self` borrow excludes;
        // `parse` copies what it keeps.
        Some(parse(
            unsafe { slice::from_raw_parts(data.cast::<u8>(), len as usize) },
            &self.text_defines,
        ))
    }

    /// Broadcasts `data` (JSON, by convention) as Comm Bus event `name` to
    /// the sim's JavaScript views: in-game panels, EFB apps, instruments.
    /// Fire and forget: nothing tells whether a view is listening.
    pub fn call_comm_bus_js(&mut self, name: &str, data: &str) -> Result<(), Error> {
        let (name, data) = (c_string(name)?, c_string(data)?);
        // SAFETY: open handle; `data` holds `comm_bus_len` bytes, its NUL included;
        // both strings outlive the call.
        let hr = unsafe {
            ffi::call_comm_bus_event(
                self.handle,
                name.as_ptr(),
                ffi::COMM_BUS_BROADCAST_TO_JS,
                comm_bus_len(&data),
                data.as_ptr(),
            )
        };
        check("SimConnect_CallCommBusEvent", hr)
    }

    /// Delivers Comm Bus event `name`, as the sim's views send it
    /// (`callSimConnect`), as `Recv::CommBus` with `event_id`.
    pub fn subscribe_comm_bus(&mut self, event_id: u32, name: &str) -> Result<(), Error> {
        let name = c_string(name)?;
        // SAFETY: open handle and a NUL-terminated name that outlives the call.
        let hr = unsafe { ffi::subscribe_to_comm_bus_event(self.handle, event_id, name.as_ptr()) };
        check("SimConnect_SubscribeToCommBusEvent", hr)
    }

    fn last_sent_packet_id(&self) -> Result<u32, Error> {
        let mut id = 0;
        // SAFETY: valid out-pointer on an open handle.
        let hr = unsafe { ffi::get_last_sent_packet_id(self.handle, &mut id) };
        check("SimConnect_GetLastSentPacketID", hr)?;
        Ok(id)
    }
}

impl Requests for SimConnect {
    /// Appends one field (or `OPEN x` / `CLOSE x`) to a facility definition.
    /// Returns the send id, which an `Exception` carries if it is rejected.
    fn add_facility_field(&mut self, define_id: u32, field: &str) -> Result<u32, Error> {
        let field = c_string(field)?;
        // SAFETY: open handle and a NUL-terminated name that outlives the call.
        let hr = unsafe { ffi::add_to_facility_definition(self.handle, define_id, field.as_ptr()) };
        check("SimConnect_AddToFacilityDefinition", hr)?;
        self.last_sent_packet_id()
    }

    /// Requests one facility through a definition; replies arrive as
    /// `Recv::Facility` data nodes then an end. Returns the send id.
    /// `region` may be empty but is always passed: the header's `= ""`
    /// default is C++ only and does not exist at the DLL boundary.
    fn request_facility_data(
        &mut self,
        define_id: u32,
        request_id: u32,
        icao: &str,
        region: &str,
    ) -> Result<u32, Error> {
        let (icao, region) = (c_string(icao)?, c_string(region)?);
        // SAFETY: open handle and NUL-terminated strings that outlive the call.
        let hr = unsafe {
            ffi::request_facility_data(
                self.handle,
                define_id,
                request_id,
                icao.as_ptr(),
                region.as_ptr(),
            )
        };
        check("SimConnect_RequestFacilityData", hr)?;
        self.last_sent_packet_id()
    }

    /// Requests the airports of the simulator's reality bubble (the area
    /// loaded around the user aircraft) as `Recv::Facility` list chunks.
    fn request_airports_nearby(&mut self, request_id: u32) -> Result<(), Error> {
        // SAFETY: open handle; scalar arguments only.
        let hr = unsafe {
            ffi::request_facilities_list_ex1(
                self.handle,
                ffi::FACILITY_LIST_TYPE_AIRPORT,
                request_id,
            )
        };
        check("SimConnect_RequestFacilitiesList_EX1", hr)
    }
}

impl Drop for SimConnect {
    fn drop(&mut self) {
        // SAFETY: the handle is open and never used after this call.
        unsafe { ffi::close(self.handle) };
    }
}

fn check(func: &'static str, hr: HResult) -> Result<(), Error> {
    if hr < 0 {
        Err(Error::Call { func, hr })
    } else {
        Ok(())
    }
}

fn c_string(s: &str) -> Result<CString, Error> {
    CString::new(s).map_err(|_| Error::InvalidName(s.to_owned()))
}

/// The Comm Bus buffer size of `data`: its bytes and the NUL, so the
/// receiving view gets a terminated string. Payloads are a few KiB.
fn comm_bus_len(data: &CString) -> u32 {
    data.as_bytes_with_nul().len() as u32
}

/// Decodes one received message; data of a definition in `text_defines`
/// decodes as strings. Every read is bounds-checked against the buffer, so a
/// truncated message becomes `Other` instead of a bad read.
fn parse(bytes: &[u8], text_defines: &HashSet<u32>) -> Recv {
    let Some(header) = read::<ffi::Recv>(bytes, 0) else {
        return Recv::Other { id: 0 };
    };
    let id = header.id;
    let decoded = match id {
        ffi::recv_id::OPEN => read::<ffi::RecvOpen>(bytes, 0).map(|m| Recv::Open {
            simconnect_version: m.simconnect_version,
            simconnect_build: m.simconnect_build,
        }),
        ffi::recv_id::QUIT => Some(Recv::Quit),
        ffi::recv_id::EXCEPTION => read::<ffi::RecvException>(bytes, 0).map(|m| Recv::Exception {
            code: m.exception,
            send_id: m.send_id,
            index: m.index,
        }),
        ffi::recv_id::EVENT => read::<ffi::RecvEvent>(bytes, 0).map(|m| Recv::Event {
            id: m.event_id,
            data: m.data,
        }),
        ffi::recv_id::EVENT_FILENAME => {
            read::<ffi::RecvEventFilename>(bytes, 0).map(|m| Recv::EventFilename {
                id: m.base.event_id,
                file: c_text(&m.file_name),
            })
        }
        ffi::recv_id::FLOW_EVENT => read::<ffi::RecvFlowEvent>(bytes, 0).map(|m| Recv::Flow {
            event: m.flow_event,
            flt_path: c_text(&m.flt_path),
        }),
        ffi::recv_id::SIMOBJECT_DATA => read::<ffi::RecvSimObjectData>(bytes, 0).and_then(|m| {
            let (start, count) = (size_of::<ffi::RecvSimObjectData>(), m.define_count as usize);
            let request_id = m.request_id;
            Some(if text_defines.contains(&{ m.define_id }) {
                let len = ffi::STRING256_LEN;
                let values = (0..count)
                    .map(|i| Some(text(bytes.get(start + i * len..start + (i + 1) * len)?)))
                    .collect::<Option<Vec<_>>>()?;
                Recv::Text { request_id, values }
            } else {
                let values = (0..count)
                    .map(|i| read::<f64>(bytes, start + i * size_of::<f64>()))
                    .collect::<Option<Vec<_>>>()?;
                Recv::Data { request_id, values }
            })
        }),
        ffi::recv_id::COMM_BUS => read::<ffi::RecvCommBus>(bytes, 0).and_then(|m| {
            // The record size, not the buffer length, bounds the payload.
            let data = bytes.get(size_of::<ffi::RecvCommBus>()..m.base.size as usize)?;
            Some(Recv::CommBus {
                event_id: m.event_id,
                data: text(data),
            })
        }),
        ffi::recv_id::AIRPORT_LIST
        | ffi::recv_id::FACILITY_DATA
        | ffi::recv_id::FACILITY_DATA_END => parse_facility(id, bytes).map(Recv::Facility),
        _ => None,
    };
    decoded.unwrap_or(Recv::Other { id })
}

/// Decodes a facility reply with message id `id`; `None` if truncated.
fn parse_facility(id: u32, bytes: &[u8]) -> Option<Facility> {
    match id {
        ffi::recv_id::AIRPORT_LIST => {
            let m = read::<ffi::RecvFacilitiesList>(bytes, 0)?;
            let start = size_of::<ffi::RecvFacilitiesList>();
            let entry = size_of::<ffi::FacilityAirport>();
            let airports = (0..m.array_size as usize)
                .map(|i| {
                    let a = read::<ffi::FacilityAirport>(bytes, start + i * entry)?;
                    Some(AirportListEntry {
                        ident: c_text(&a.ident),
                        region: c_text(&a.region),
                        lat_deg: a.latitude,
                        lon_deg: a.longitude,
                        elevation_m: a.altitude,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Facility::AirportList {
                request_id: m.request_id,
                entry_number: m.entry_number,
                out_of: m.out_of,
                airports,
            })
        }
        ffi::recv_id::FACILITY_DATA => {
            let m = read::<ffi::RecvFacilityData>(bytes, 0)?;
            // The record size, not the buffer length, bounds the payload.
            let payload = bytes.get(size_of::<ffi::RecvFacilityData>()..m.base.size as usize)?;
            Some(Facility::Data(FacilityData {
                request_id: m.user_request_id,
                unique_id: m.unique_request_id,
                parent_id: m.parent_unique_request_id,
                data_type: m.data_type,
                is_list_item: m.is_list_item,
                item_index: m.item_index,
                list_size: m.list_size,
                payload: payload.to_vec(),
            }))
        }
        ffi::recv_id::FACILITY_DATA_END => {
            read::<ffi::RecvFacilityDataEnd>(bytes, 0).map(|m| Facility::End {
                request_id: m.request_id,
            })
        }
        _ => None,
    }
}

/// A fixed-size, NUL-terminated C string field as text.
fn c_text(chars: &[c_char]) -> String {
    text(&chars.iter().map(|&c| c as u8).collect::<Vec<_>>())
}

/// A fixed-size string field up to its first NUL (the whole field if it has
/// none); bytes that are not UTF-8 become U+FFFD.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Copies a `T` out of `bytes` at `offset`, if it fits. Only used with the
/// plain-data `ffi` structs and `f64`, for which every bit pattern is valid.
fn read<T>(bytes: &[u8], offset: usize) -> Option<T> {
    let src = bytes.get(offset..offset.checked_add(size_of::<T>())?)?;
    // SAFETY: `src` holds exactly `size_of::<T>()` bytes; unaligned read.
    Some(unsafe { ptr::read_unaligned(src.as_ptr().cast::<T>()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// The text definition `decode` knows; float data uses definition 1.
    const TEXT_DEFINE: u32 = 2;

    fn decode(bytes: &[u8]) -> Recv {
        parse(bytes, &HashSet::from([TEXT_DEFINE]))
    }

    /// Data of request 7 for `define_id`: `count` values packed in `payload`.
    fn simobject_data(define_id: u32, count: usize, payload: &[u8]) -> Vec<u8> {
        let size = 40 + payload.len() as u32;
        let id = ffi::recv_id::SIMOBJECT_DATA;
        let mut bytes = words(&[size, 6, id, 7, 0, define_id, 0, 1, 1, count as u32]);
        bytes.extend(payload);
        bytes
    }

    fn data_message(values: &[f64]) -> Vec<u8> {
        let payload: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        simobject_data(1, values.len(), &payload)
    }

    fn text_message(values: &[&str]) -> Vec<u8> {
        let mut payload = Vec::new();
        for v in values {
            let mut field = [0; ffi::STRING256_LEN];
            field[..v.len()].copy_from_slice(v.as_bytes());
            payload.extend(field);
        }
        simobject_data(TEXT_DEFINE, values.len(), &payload)
    }

    #[test]
    fn decodes_text_definitions_as_strings() {
        let values = [
            "Cessna Skyhawk",
            "TT:ATCCOM.AC_MODEL_C172.0.text",
            "",
            "D-EGEB",
        ];
        assert_eq!(
            decode(&text_message(&values)),
            Recv::Text {
                request_id: 7,
                values: values.map(String::from).into(),
            }
        );
        // A field filled to the last byte has no NUL and is kept whole.
        let full = "A".repeat(ffi::STRING256_LEN);
        assert!(matches!(
            decode(&text_message(&[&full])),
            Recv::Text { values, .. } if values == [full]
        ));
    }

    #[test]
    fn truncated_text_is_not_read_past_its_end() {
        let mut bytes = text_message(&["C172", "N172SP"]);
        bytes.truncate(bytes.len() - 1);
        let id = ffi::recv_id::SIMOBJECT_DATA;
        assert_eq!(decode(&bytes), Recv::Other { id });
    }

    #[test]
    fn decodes_simobject_data_in_definition_order() {
        let msg = decode(&data_message(&[1500.0, -2.5]));
        assert_eq!(
            msg,
            Recv::Data {
                request_id: 7,
                values: vec![1500.0, -2.5]
            }
        );
    }

    #[test]
    fn truncated_message_is_not_read_past_its_end() {
        let mut bytes = data_message(&[1.0, 2.0]);
        bytes.truncate(bytes.len() - 1);
        assert_eq!(
            decode(&bytes),
            Recv::Other {
                id: ffi::recv_id::SIMOBJECT_DATA
            }
        );
        assert_eq!(decode(&[0; 5]), Recv::Other { id: 0 });
    }

    fn airport_list(entries: &[(&str, f64)]) -> Vec<u8> {
        let size = 28 + 36 * entries.len() as u32;
        let n = entries.len() as u32;
        let mut bytes = words(&[size, 6, ffi::recv_id::AIRPORT_LIST, 3, n, 1, 2]);
        for (ident, lat) in entries {
            let mut text = [0u8; 12];
            text[..ident.len()].copy_from_slice(ident.as_bytes());
            bytes.extend(text);
            bytes.extend([*lat, 27.0, 120.0].iter().flat_map(|v| v.to_le_bytes()));
        }
        bytes
    }

    #[test]
    fn decodes_airport_list_chunk() {
        let msg = decode(&airport_list(&[("LTBJ", 38.29), ("LTFB", 37.95)]));
        let Recv::Facility(Facility::AirportList {
            request_id: 3,
            entry_number: 1,
            out_of: 2,
            airports,
        }) = msg
        else {
            panic!("{msg:?}");
        };
        assert_eq!(airports[0].ident, "LTBJ");
        assert_eq!(
            airports[1],
            AirportListEntry {
                ident: "LTFB".into(),
                region: String::new(),
                lat_deg: 37.95,
                lon_deg: 27.0,
                elevation_m: 120.0,
            }
        );
    }

    #[test]
    fn airport_list_with_zero_entries_or_truncated() {
        assert!(matches!(
            decode(&airport_list(&[])),
            Recv::Facility(Facility::AirportList { airports, .. }) if airports.is_empty()
        ));
        let mut bytes = airport_list(&[("LTBJ", 38.29)]);
        bytes.truncate(bytes.len() - 1);
        assert_eq!(
            decode(&bytes),
            Recv::Other {
                id: ffi::recv_id::AIRPORT_LIST
            }
        );
    }

    fn facility_data(payload: &[u8]) -> Vec<u8> {
        let size = 40 + payload.len() as u32;
        let id = ffi::recv_id::FACILITY_DATA;
        let mut bytes = words(&[size, 6, id, 5, 11, 10, 1, 0, 0, 0]);
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn decodes_facility_data_payload_by_record_size() {
        let mut bytes = facility_data(&[1, 2, 3, 4]);
        bytes.extend([0xEE; 4]); // buffer slack past the record is not payload
        assert_eq!(
            decode(&bytes),
            Recv::Facility(Facility::Data(FacilityData {
                request_id: 5,
                unique_id: 11,
                parent_id: 10,
                data_type: 1,
                is_list_item: 0,
                item_index: 0,
                list_size: 0,
                payload: vec![1, 2, 3, 4],
            }))
        );
        assert!(matches!(
            decode(&facility_data(&[])),
            Recv::Facility(Facility::Data(d)) if d.payload.is_empty()
        ));
    }

    #[test]
    fn truncated_facility_data_and_end_are_not_decoded() {
        let mut bytes = facility_data(&[1, 2, 3, 4]);
        bytes.truncate(bytes.len() - 1);
        let other = |id| Recv::Other { id };
        assert_eq!(decode(&bytes), other(ffi::recv_id::FACILITY_DATA));
        let end = words(&[16, 6, ffi::recv_id::FACILITY_DATA_END, 5]);
        assert_eq!(
            decode(&end),
            Recv::Facility(Facility::End { request_id: 5 })
        );
        assert_eq!(decode(&end[..15]), other(ffi::recv_id::FACILITY_DATA_END));
    }

    #[test]
    fn comm_bus_buffer_counts_the_terminator() {
        assert_eq!(comm_bus_len(&c_string(r#"{"v":1}"#).unwrap()), 8);
        assert_eq!(comm_bus_len(&c_string("").unwrap()), 1);
        assert!(matches!(c_string("a b"), Err(Error::InvalidName(_))));
    }

    fn comm_bus(event_id: u32, payload: &[u8]) -> Vec<u8> {
        let size = 32 + payload.len() as u32;
        let len = payload.len() as u32;
        let mut bytes = words(&[size, 6, ffi::recv_id::COMM_BUS, 0, len, 0, 1, event_id]);
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn decodes_comm_bus_payload_up_to_its_nul() {
        let cmd = r#"{"v":2,"cmd":"mark"}"#;
        let expected = Recv::CommBus {
            event_id: 9,
            data: cmd.into(),
        };
        // With or without terminators, and slack past the record.
        assert_eq!(decode(&comm_bus(9, cmd.as_bytes())), expected);
        let mut bytes = comm_bus(9, format!("{cmd}  ").as_bytes());
        bytes.extend([0xEE; 4]);
        assert_eq!(decode(&bytes), expected);
        let mut short = comm_bus(9, cmd.as_bytes());
        short.truncate(31);
        assert_eq!(decode(&short), Recv::Other { id: 43 });
    }

    #[test]
    fn decodes_exception_with_send_id() {
        let msg = decode(&words(&[24, 6, ffi::recv_id::EXCEPTION, 7, 42, 1]));
        assert_eq!(
            msg,
            Recv::Exception {
                code: 7,
                send_id: 42,
                index: 1
            }
        );
    }
}
