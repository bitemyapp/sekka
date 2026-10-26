//! The table's columns: which each lens shows, what each says, and how
//! they sort.

use std::cmp::Ordering;

use crate::model::{self, Frame, Group, Lens, Process, Usage};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Col {
    Name,
    Cpu,
    CpuTime,
    Threads,
    Wakeups,
    Memory,
    Private,
    Resident,
    Shared,
    Swap,
    Power,
    Average,
    Energy,
    Gpu,
    Video,
    Vram,
    GpuShared,
    Read,
    Write,
    ReadTotal,
    WriteTotal,
    Pid,
    User,
}

use Col::*;

impl Col {
    pub fn title(self) -> &'static str {
        match self {
            Name => "Name",
            Cpu => "% CPU",
            CpuTime => "CPU Time",
            Threads => "Threads",
            Wakeups => "Wake-ups",
            Memory => "Memory",
            Private => "Private",
            Resident => "Resident",
            Shared => "Shared",
            Swap => "Swap",
            Power => "Power",
            Average => "Average",
            Energy => "Energy Used",
            Gpu => "% GPU",
            Video => "% Video",
            Vram => "Video Memory",
            GpuShared => "Shared GPU",
            Read => "Reading",
            Write => "Writing",
            ReadTotal => "Read",
            WriteTotal => "Written",
            Pid => "PID",
            User => "User",
        }
    }

    /// What the column means, for its header's tooltip and the inspector.
    pub fn about(self) -> &'static str {
        match self {
            Name => "The application or process",
            Cpu => "Share of one processor core over the last reading; a busy application can pass 100",
            CpuTime => "Processor time used since it started",
            Threads => "Threads of execution",
            Wakeups => "Times a second it gave up the processor to wait: each keeps a core from resting",
            Memory => "Its own memory plus its share of what it shares (proportional set size)",
            Private => "Memory no other process shares",
            Resident => "All memory in RAM it can reach, shared included",
            Shared => "Memory it shares with other processes",
            Swap => "Memory moved out to swap",
            Power => "Estimated power now, from the meters, by its share of the processor",
            Average => "Estimated average power while Myaku has been open",
            Energy => "Estimated energy used while Myaku has been open",
            Gpu => "Busiest drawing or compute engine of the graphics cards it uses",
            Video => "Busiest video encoding or decoding engine",
            Vram => "Graphics memory it holds",
            GpuShared => "System memory mapped for the graphics card",
            Read | Write => "Storage read or written each second",
            ReadTotal | WriteTotal => "Storage read or written since it started",
            Pid => "Process ID",
            User => "Who runs it",
        }
    }

    pub fn width(self) -> f64 {
        match self {
            Name => 300.0,
            Pid => 64.0,
            User => 84.0,
            Threads => 70.0,
            Cpu | Gpu | Video => 72.0,
            _ => 96.0,
        }
    }

    /// Its width beside the inspector.
    pub fn narrow_width(self) -> f64 {
        match self {
            Name => 210.0,
            Pid => 56.0,
            Cpu | Gpu | Video => 64.0,
            Threads => 62.0,
            _ => 80.0,
        }
    }

    pub fn numeric(self) -> bool {
        !matches!(self, Name | User)
    }
}

/// The columns of a lens, in order.
pub fn for_lens(lens: Lens) -> &'static [Col] {
    match lens {
        Lens::Cpu => &[Name, Cpu, CpuTime, Threads, Wakeups, Pid, User],
        Lens::Memory => &[Name, Memory, Private, Resident, Shared, Swap, Pid, User],
        Lens::Energy => &[Name, Power, Average, Energy, Cpu, Gpu, Pid],
        Lens::Gpu => &[Name, Gpu, Video, Vram, GpuShared, Pid, User],
        Lens::Disk => &[Name, Read, Write, ReadTotal, WriteTotal, Pid, User],
    }
}

pub fn default_sort(lens: Lens) -> Col {
    match lens {
        Lens::Cpu => Cpu,
        Lens::Memory => Memory,
        Lens::Energy => Power,
        Lens::Gpu => Gpu,
        Lens::Disk => Read,
    }
}

/// A usage figure as a number for sorting (`None`: not known).
fn number(col: Col, u: &Usage) -> Option<f64> {
    Some(match col {
        Cpu => u.cpu,
        CpuTime => u.cpu_time,
        Threads => u.threads as f64,
        Wakeups => u.wakeups,
        Memory => u.memory() as f64,
        Private => u.private? as f64,
        Resident => u.resident as f64,
        Shared => u.shared? as f64,
        Swap => u.swap as f64,
        Power => u.power?,
        Average => u.average_power()?,
        Energy => u.energy,
        Gpu => u.gpu,
        Video => u.video,
        Vram => u.vram as f64,
        GpuShared => u.gpu_shared as f64,
        Read => u.read_rate?,
        Write => u.write_rate?,
        ReadTotal => u.read_total? as f64,
        WriteTotal => u.write_total? as f64,
        _ => return None,
    })
}

/// Unknowns last, whichever way.
fn order_numbers(a: Option<f64>, b: Option<f64>, asc: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            let o = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if asc { o } else { o.reverse() }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn compare_process(col: Col, a: &Process, b: &Process, asc: bool) -> Ordering {
    let o = match col {
        Name => {
            let o = a.name.to_lowercase().cmp(&b.name.to_lowercase());
            if asc { o } else { o.reverse() }
        }
        User => {
            let o = a.user.cmp(&b.user);
            if asc { o } else { o.reverse() }
        }
        Pid => {
            let o = a.pid.cmp(&b.pid);
            if asc { o } else { o.reverse() }
        }
        c => order_numbers(number(c, &a.usage), number(c, &b.usage), asc),
    };
    o.then(a.pid.cmp(&b.pid))
}

pub fn compare_group(col: Col, a: &Group, b: &Group, f: &Frame, asc: bool) -> Ordering {
    let first = |g: &Group| &f.processes[g.members[0]];
    let o = match col {
        Name => {
            let o = crate::app::kind_rank(a.kind)
                .cmp(&crate::app::kind_rank(b.kind))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            if asc { o } else { o.reverse() }
        }
        User => {
            let o = first(a).user.cmp(&first(b).user);
            if asc { o } else { o.reverse() }
        }
        Pid => {
            let o = first(a).pid.cmp(&first(b).pid);
            if asc { o } else { o.reverse() }
        }
        c => order_numbers(number(c, &a.usage), number(c, &b.usage), asc),
    };
    o.then(a.key.cmp(&b.key))
}

/// What a cell says (`None`: not known, a dash).
pub fn text(col: Col, u: &Usage, pid: Option<u32>, user: Option<&str>) -> Option<String> {
    Some(match col {
        Name => return None,
        Cpu => model::percent(u.cpu),
        CpuTime => model::cpu_time(u.cpu_time),
        Threads => model::grouped(u.threads),
        Wakeups => model::grouped(u.wakeups.round() as u64),
        Memory => model::bytes(u.memory()),
        Private => model::bytes(u.private?),
        Resident => model::bytes(u.resident),
        Shared => model::bytes(u.shared?),
        Swap => model::bytes(u.swap),
        Power => model::watts(u.power?),
        Average => model::watts(u.average_power()?),
        Energy => model::energy(u.energy),
        Gpu => model::percent(u.gpu),
        Video => model::percent(u.video),
        Vram => model::bytes(u.vram),
        GpuShared => model::bytes(u.gpu_shared),
        Read => model::rate(u.read_rate?),
        Write => model::rate(u.write_rate?),
        ReadTotal => model::bytes(u.read_total?),
        WriteTotal => model::bytes(u.write_total?),
        Pid => pid?.to_string(),
        User => user?.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lens_leads_with_the_name_and_sorts_by_its_own() {
        for l in Lens::ALL {
            assert_eq!(for_lens(l)[0], Name);
            assert!(for_lens(l).contains(&default_sort(l)));
        }
    }

    #[test]
    fn unknowns_sort_last_either_way() {
        let a = Process { pid: 1, usage: Usage { private: Some(5), ..Default::default() }, ..Default::default() };
        let b = Process { pid: 2, ..Default::default() };
        assert_eq!(compare_process(Private, &a, &b, false), Ordering::Less);
        assert_eq!(compare_process(Private, &a, &b, true), Ordering::Less);
    }
}
