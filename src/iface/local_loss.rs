//! The tags an interface puts on the TCP segments it sends, so that a device's report of a
//! segment lost before it left the link layer finds its socket.

use crate::config::IFACE_LOCAL_LOSS_TAG_COUNT;
use crate::iface::SocketHandle;
use crate::wire::TcpSeqNumber;

#[derive(Debug, Clone, Copy)]
struct Tag {
    id: u32,
    handle: SocketHandle,
    seq: TcpSeqNumber,
}

/// The most recent segments' tags, each in the slot of its ID: a later segment takes an older
/// one's slot, and a report for the older one is then ignored.
#[derive(Debug)]
pub(crate) struct LossTags {
    tags: [Option<Tag>; IFACE_LOCAL_LOSS_TAG_COUNT],
    next_id: u32,
}

impl LossTags {
    pub(crate) fn new() -> Self {
        Self {
            tags: [None; IFACE_LOCAL_LOSS_TAG_COUNT],
            next_id: 0,
        }
    }

    /// Tags the segment that starts at `seq` from the socket `handle`, and returns its ID.
    pub(crate) fn tag(&mut self, handle: SocketHandle, seq: TcpSeqNumber) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.tags[Self::slot(id)] = Some(Tag { id, handle, seq });
        id
    }

    /// The socket and sequence number of the segment with ID `id`, if it's still known; it
    /// is forgotten, so that it's reported at most once.
    pub(crate) fn take(&mut self, id: u32) -> Option<(SocketHandle, TcpSeqNumber)> {
        let slot = &mut self.tags[Self::slot(id)];
        match *slot {
            Some(tag) if tag.id == id => {
                *slot = None;
                Some((tag.handle, tag.seq))
            }
            _ => None,
        }
    }

    fn slot(id: u32) -> usize {
        id as usize % IFACE_LOCAL_LOSS_TAG_COUNT
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_tag_is_taken_once() {
        let mut tags = LossTags::new();
        let handle = SocketHandle::default();
        let id = tags.tag(handle, TcpSeqNumber(100));
        assert_eq!(tags.take(id), Some((handle, TcpSeqNumber(100))));
        assert_eq!(tags.take(id), None);
    }

    #[test]
    fn a_later_tag_overwrites_an_older_one_in_its_slot() {
        let mut tags = LossTags::new();
        let handle = SocketHandle::default();
        let first = tags.tag(handle, TcpSeqNumber(0));
        for seq in 1..=IFACE_LOCAL_LOSS_TAG_COUNT {
            tags.tag(handle, TcpSeqNumber(seq as i32));
        }
        assert_eq!(tags.take(first), None);
        assert_eq!(tags.take(first + 1), Some((handle, TcpSeqNumber(1))));
    }
}
