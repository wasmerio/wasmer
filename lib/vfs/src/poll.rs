#[cfg(unix)]
fn poll_event_set_to_platform_poll_events(mut pes: PollEventSet) -> i16 {
    let mut out = 0;
    for i in 0..16 {
        out |= match PollEvent::from_i16(pes & (1 << i)) {
            Some(PollEvent::PollIn) => libc::POLLIN,
            Some(PollEvent::PollOut) => libc::POLLOUT,
            Some(PollEvent::PollError) => libc::POLLERR,
            Some(PollEvent::PollHangUp) => libc::POLLHUP,
            Some(PollEvent::PollInvalid) => libc::POLLNVAL,
            _ => 0,
        };
        pes &= !(1 << i);
    }
    out
}

#[cfg(unix)]
fn platform_poll_events_to_pollevent_set(mut num: i16) -> PollEventSet {
    let mut peb = PollEventBuilder::new();
    for i in 0..16 {
        peb = match num & (1 << i) {
            libc::POLLIN => peb.add(PollEvent::PollIn),
            libc::POLLOUT => peb.add(PollEvent::PollOut),
            libc::POLLERR => peb.add(PollEvent::PollError),
            libc::POLLHUP => peb.add(PollEvent::PollHangUp),
            libc::POLLNVAL => peb.add(PollEvent::PollInvalid),
            _ => peb,
        };
        num &= !(1 << i);
    }
    peb.build()
}