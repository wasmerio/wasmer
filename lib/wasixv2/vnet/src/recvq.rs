//! A TCP receive queue and its urgent mark, as `tcp_recvmsg`, `tcp_recv_urg`, `tcp_poll` and
//! `tcp_ioctl` read a `tcp_sock`: `copied_seq` and `rcv_nxt` bound the bytes queued, `urg_data`
//! and `urg_seq` describe the urgent byte.
//!
//! Urgent data is BSD's (`tcp_stdurg` off): a send with `MSG_OOB` makes the last byte it sends
//! urgent, one byte per mark, and a newer mark replaces an older one. Unless the reader set
//! `SO_OOBINLINE`, the byte leaves the stream: reads stop in front of it and step over it, and
//! only `recv(MSG_OOB)` reads it. Here a mark always arrives with its byte, so it is never ahead
//! of the data (`TCP_URG_NOTYET`).

use std::collections::VecDeque;

use wasixv2_abi::Errno;

/// `urg_data`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Urg {
    /// No mark, or the reader went past it.
    #[default]
    None,
    /// `TCP_URG_VALID`: the urgent byte, which `MSG_OOB` has not read yet.
    Valid(u8),
    /// `TCP_URG_READ`: `MSG_OOB` read it; the mark stays until the stream passes it.
    Read,
}

/// The bytes queued for one side of a TCP connection and its urgent mark.
#[derive(Debug, Default)]
pub struct RecvQueue {
    data: VecDeque<u8>,
    /// `copied_seq`: where `data[0]` is in the stream.
    copied: u64,
    urg: Urg,
    /// `urg_seq`: where the urgent byte is in the stream, while there is a mark.
    urg_seq: u64,
}

impl RecvQueue {
    /// Bytes queued (`rcv_nxt - copied_seq`), the urgent byte included.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Drops everything queued and the mark (`tcp_close` purging the queue, `tcp_disconnect`).
    pub fn clear(&mut self) {
        self.copied += self.data.len() as u64;
        self.data.clear();
        self.urg = Urg::None;
    }

    /// Queues ordinary data.
    pub fn push(&mut self, data: &[u8]) {
        self.data.extend(data);
    }

    /// Queues `data`, whose last byte a send with `MSG_OOB` made urgent: the new mark
    /// (`tcp_check_urg`, then `tcp_urg` finding the byte). An earlier urgent byte still at the
    /// head of a stream not read inline goes first, so it is never read as data.
    pub fn push_urgent(&mut self, data: &[u8], inline: bool) {
        let Some(&byte) = data.last() else {
            return;
        };
        if self.at_mark() && !inline && !self.data.is_empty() {
            self.data.pop_front();
            self.copied += 1;
        }
        self.data.extend(data);
        self.urg_seq = self.copied + self.data.len() as u64 - 1;
        self.urg = Urg::Valid(byte);
    }

    /// Whether the urgent byte waits for `recv(MSG_OOB)` (`POLLPRI`).
    pub fn urgent(&self) -> bool {
        matches!(self.urg, Urg::Valid(_))
    }

    /// `SIOCATMARK`: the next byte of the stream is the urgent one.
    pub fn at_mark(&self) -> bool {
        self.urg != Urg::None && self.urg_seq == self.copied
    }

    /// Whether a read finds `lowat` bytes (`tcp_poll`'s target): an urgent byte at the head
    /// that is not read inline does not count.
    pub fn readable(&self, lowat: usize, inline: bool) -> bool {
        let target = lowat.max(1) + usize::from(self.at_mark() && !inline);
        self.data.len() >= target
    }

    /// `SIOCINQ` (`tcp_inq`): what is queued, or only what is ahead of an urgent byte that is
    /// not read inline.
    pub fn inq(&self, inline: bool) -> usize {
        let end = self.copied + self.data.len() as u64;
        if inline || self.urg == Urg::None || self.urg_seq < self.copied || self.urg_seq >= end {
            return self.data.len();
        }
        (self.urg_seq - self.copied) as usize
    }

    /// Reads into `buf` as `tcp_recvmsg` does: never past the mark once something was read, and
    /// over the urgent byte at the head unless `inline`. `peek` leaves everything queued. Returns
    /// the bytes copied; 0 for a non-empty `buf` means there was nothing to read, though an
    /// urgent byte may have been stepped over and taken.
    pub fn read(&mut self, buf: &mut [u8], peek: bool, inline: bool) -> usize {
        let end = self.copied + self.data.len() as u64;
        let marked = self.urg != Urg::None;
        let mut seq = self.copied;
        let mut copied = 0;
        while copied < buf.len() && seq < end {
            if marked && self.urg_seq == seq && copied > 0 {
                break;
            }
            let mut offset = (seq - self.copied) as usize;
            let mut used = ((end - seq) as usize).min(buf.len() - copied);
            let urg_offset = self.urg_seq.wrapping_sub(seq);
            if marked && urg_offset < used as u64 {
                if urg_offset != 0 {
                    used = urg_offset as usize;
                } else if !inline {
                    seq += 1;
                    offset += 1;
                    used -= 1;
                }
            }
            // tcp_recvmsg_locked copies one contiguous skb fragment at a time.
            // A VecDeque has at most two; keep the urgent-boundary logic above.
            let (first, second) = self.data.as_slices();
            let n = used.min(first.len().saturating_sub(offset));
            if n != 0 {
                buf[copied..copied + n].copy_from_slice(&first[offset..offset + n]);
            }
            let at = offset.saturating_sub(first.len());
            buf[copied + n..copied + used].copy_from_slice(&second[at..at + used - n]);
            copied += used;
            seq += used as u64;
        }
        if !peek {
            self.data.drain(..(seq - self.copied) as usize);
            self.copied = seq;
            if marked && self.copied > self.urg_seq {
                self.urg = Urg::None;
            }
        }
        copied
    }

    /// `tcp_recv_urg`: the urgent byte, left for another read with `peek`. `EINVAL` when there
    /// is none to read (none came, `MSG_OOB` read it, the stream passed it, or it is inline),
    /// then `ENOTCONN` for a connection that ended without the peer's FIN (`closed`).
    pub fn recv_urgent(&mut self, peek: bool, inline: bool, closed: bool) -> Result<u8, Errno> {
        let Urg::Valid(byte) = self.urg else {
            return Err(Errno::EINVAL);
        };
        if inline {
            return Err(Errno::EINVAL);
        }
        if closed {
            return Err(Errno::ENOTCONN);
        }
        if !peek {
            self.urg = Urg::Read;
        }
        Ok(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(q: &mut RecvQueue, n: usize, peek: bool, inline: bool) -> Vec<u8> {
        let mut buf = vec![0; n];
        let got = q.read(&mut buf, peek, inline);
        buf.truncate(got);
        buf
    }

    #[test]
    fn wrapped_reads_preserve_peeks_and_the_urgent_mark() {
        let mut q = RecvQueue::default();
        q.push(b"abcd");
        assert_eq!(read(&mut q, 3, false, false), b"abc");
        q.push(b"efghi");
        q.push_urgent(b"j!", false);
        assert!(!q.data.as_slices().1.is_empty());
        assert_eq!(read(&mut q, 16, true, false), b"defghij");
        assert_eq!(q.len(), 8);
        assert_eq!(q.recv_urgent(true, false, false), Ok(b'!'));
        assert_eq!(read(&mut q, 16, false, false), b"defghij");
        assert!(q.at_mark());
        assert_eq!(q.recv_urgent(false, false, false), Ok(b'!'));
        assert_eq!(read(&mut q, 1, false, false), b"");
        assert!(q.is_empty());
    }

    #[test]
    fn reads_stop_at_the_mark_and_step_over_the_urgent_byte() {
        let mut q = RecvQueue::default();
        q.push(b"ab");
        q.push_urgent(b"cX", false);
        q.push(b"de");
        assert!(q.urgent());
        assert!(!q.at_mark());
        assert_eq!(q.inq(false), 3);
        assert_eq!(q.inq(true), 6);
        assert_eq!(read(&mut q, 16, false, false), b"abc");
        assert!(q.at_mark());
        assert_eq!(q.inq(false), 0);
        assert_eq!(q.recv_urgent(true, false, false), Ok(b'X'));
        assert_eq!(q.recv_urgent(false, false, false), Ok(b'X'));
        assert_eq!(q.recv_urgent(false, false, false), Err(Errno::EINVAL));
        assert!(!q.urgent());
        assert!(q.at_mark());
        assert_eq!(read(&mut q, 16, false, false), b"de");
        assert!(!q.at_mark());
        assert!(q.is_empty());
    }

    #[test]
    fn reading_past_the_mark_loses_the_urgent_byte() {
        let mut q = RecvQueue::default();
        q.push_urgent(b"X", false);
        q.push(b"a");
        assert!(q.at_mark());
        assert!(q.readable(1, false));
        assert_eq!(read(&mut q, 16, true, false), b"a");
        assert_eq!(read(&mut q, 16, false, false), b"a");
        assert_eq!(q.recv_urgent(false, false, false), Err(Errno::EINVAL));
    }

    #[test]
    fn an_urgent_byte_alone_is_not_readable() {
        let mut q = RecvQueue::default();
        q.push_urgent(b"X", false);
        assert!(!q.readable(1, false));
        assert!(q.readable(1, true));
        assert_eq!(read(&mut q, 16, true, false), b"");
        assert_eq!(q.len(), 1);
        // A read takes the byte it steps over, and the mark with it.
        assert_eq!(read(&mut q, 16, false, false), b"");
        assert!(q.is_empty());
        assert!(!q.at_mark());
    }

    #[test]
    fn inline_urgent_bytes_are_data() {
        let mut q = RecvQueue::default();
        q.push(b"a");
        q.push_urgent(b"X", true);
        q.push(b"b");
        assert_eq!(q.recv_urgent(false, true, false), Err(Errno::EINVAL));
        assert_eq!(read(&mut q, 16, false, true), b"a");
        assert!(q.at_mark());
        assert_eq!(read(&mut q, 16, false, true), b"Xb");
        assert!(!q.at_mark());
    }

    #[test]
    fn a_newer_mark_drops_an_unread_urgent_byte_at_the_head() {
        let mut q = RecvQueue::default();
        q.push_urgent(b"X", false);
        q.push(b"a");
        q.push_urgent(b"Y", false);
        assert_eq!(q.len(), 2);
        assert_eq!(read(&mut q, 16, false, false), b"a");
        assert_eq!(q.recv_urgent(false, false, false), Ok(b'Y'));
        assert_eq!(q.recv_urgent(false, false, true), Err(Errno::EINVAL));
    }

    #[test]
    fn a_closed_connection_keeps_its_urgent_byte_out_of_reach() {
        let mut q = RecvQueue::default();
        q.push_urgent(b"X", false);
        assert_eq!(q.recv_urgent(false, false, true), Err(Errno::ENOTCONN));
        assert_eq!(q.recv_urgent(false, false, false), Ok(b'X'));
    }
}
