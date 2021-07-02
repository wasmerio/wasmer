#include <stdint.h>
#include "../../../wasmer_wasi_experimental_network.h"

#define EVFILT_READ             (-1)
#define EVFILT_WRITE            (-2)
#define EVFILT_AIO              (-3)    /* attached to aio requests */
#define EVFILT_VNODE            (-4)    /* attached to vnodes */
#define EVFILT_PROC             (-5)    /* attached to struct proc */
#define EVFILT_SIGNAL           (-6)    /* attached to struct proc */
#define EVFILT_TIMER            (-7)    /* timers */
#define EVFILT_MACHPORT         (-8)    /* Mach portsets */
#define EVFILT_FS               (-9)    /* Filesystem events */
#define EVFILT_USER             (-10)   /* User events */
#define EVFILT_VM               (-12)   /* Virtual memory events */
#define EVFILT_EXCEPT           (-15)   /* Exception events */

struct kevent {
  uintptr_t ident;  /* identifier for this event */
  int16_t filter;   /* filter for event */
  uint16_t flags;   /* general flags */
  uint32_t fflags;  /* filter-specific flags */
  intptr_t data;    /* filter-specific data */
  void *udata;      /* opaque user data identifier */
};

struct kevent64_s {
  uint64_t ident;  /* identifier for this event */
  int16_t filter;  /* filter for event */
  uint16_t flags;  /* general flags */
  uint32_t fflags; /* filter-specific flags */
  int64_t data;    /* filter-specific data */
  uint64_t udata;  /* opaque user data identifier */
  uint64_t ext[2]; /* filter-specific extensions */
};


#define EV_SET(kevp, a, b, c, d, e, f) do {     \
    struct kevent *__kevp__ = (kevp);           \
    __kevp__->ident = (a);                      \
    __kevp__->filter = (b);                     \
    __kevp__->flags = (c);                      \
    __kevp__->fflags = (d);                     \
    __kevp__->data = (e);                       \
    __kevp__->udata = (f);                      \
  } while(0)

#define EV_SET64(kevp, a, b, c, d, e, f, g, h) do {     \
    struct kevent64_s *__kevp__ = (kevp);               \
    __kevp__->ident = (a);                              \
    __kevp__->filter = (b);                             \
    __kevp__->flags = (c);                              \
    __kevp__->fflags = (d);                             \
    __kevp__->data = (e);                               \
    __kevp__->udata = (f);                              \
    __kevp__->ext[0] = (g);                             \
    __kevp__->ext[1] = (h);                             \
  } while(0)

/* actions */
#define EV_ADD              0x0001      /* add event to kq (implies enable) */
#define EV_DELETE           0x0002      /* delete event from kq */
#define EV_ENABLE           0x0004      /* enable event */
#define EV_DISABLE          0x0008      /* disable event (not reported) */

/* flags */
#define EV_ONESHOT          0x0010      /* only report one occurrence */
#define EV_CLEAR            0x0020      /* clear event state after reporting */
#define EV_RECEIPT          0x0040      /* force immediate event output */
                                        /* ... with or without EV_ERROR */
                                        /* ... use KEVENT_FLAG_ERROR_EVENTS */
                                        /*     on syscalls supporting flags */

#define EV_DISPATCH         0x0080      /* disable event after reporting */
#define EV_UDATA_SPECIFIC   0x0100      /* unique kevent per udata value */

#define EV_DISPATCH2        (EV_DISPATCH | EV_UDATA_SPECIFIC)
/* ... in combination with EV_DELETE */
/* will defer delete until udata-specific */
/* event enabled. EINPROGRESS will be */
/* returned to indicate the deferral */

#define EV_VANISHED         0x0200      /* report that source has vanished  */
                                        /* ... only valid with EV_DISPATCH2 */

#define EV_SYSFLAGS         0xF000      /* reserved by system */
#define EV_FLAG0            0x1000      /* filter-specific flag */
#define EV_FLAG1            0x2000      /* filter-specific flag */

/* returned values */
#define EV_EOF              0x8000      /* EOF detected */
#define EV_ERROR            0x4000      /* error, data contains errno */

struct timespec;

int kqueue(void);
int kevent(
           int kq,
           const struct kevent *changelist,
           int nchanges,
           struct kevent *eventlist,
           int nevents,
           const struct timespec *timeout
           );

int kevent64(
             int kq,
             const struct kevent64_s *changelist,
             int nchanges,
             struct kevent64_s *eventlist,
             int nevents,
             unsigned int flags,
             const struct timespec *timeout
             );

// This is a best-effort to make `kqueue` compatible with
// `wasmer_wasi_experimental_network`.
int kqueue(void) {
  __wasi_poll_t poll = 0;
  __wasi_errno_t err = poller_create(&poll);

  if (err != 0) {
    return err;
  }

  return poll;
}

// This is a best-effort to make `kevent` compatible with
// `wasmer_wasi_experimental_network`.
//
// Note that all events are “oneshots”,i.e. they act like if the
// `EV_ONESHOT` flag was enabled.
int kevent(
           int kq,
           const struct kevent *changelist,
           int nchanges,
           struct kevent *eventlist,
           int nevents,
           const struct timespec *timeout
           )
{
  // `changelist` is not empty, so `kevent` is used to modify events.
  if (nchanges > 0) {
    for (int nth = 0; nth < nchanges; ++nth) {
      const struct kevent* change = &changelist[nth];

      __wasi_poll_event_t event = {
        .token = change->ident,
        .readable = change->filter == EVFILT_READ,
        .writable = change->filter == EVFILT_WRITE || (change->filter == EVFILT_READ && (change->flags & EV_EOF) != 0),
      };

      if ((change->flags & EV_ADD) != 0) {
        __wasi_errno_t err = poller_modify(kq, change->ident, event);

        if (err != 0) {
          return err;
        }
      }

      if ((change->flags & EV_DELETE) != 0) {
        __wasi_errno_t err = poller_delete(kq, change->ident);

        if (err != 0) {
          return err;
        }
      }
    }
  }
  // `changelist` is empty, so `kevent` is used to wait on new events.
  // Note from kqueue(2):
  //
  // > When `nevents` is zero, `kevent()` will return immediately even if
  // > there is a timeout specified unlike select(2).
  //
  // Consequently, we check that `nevents` is non-zero to run `poller_wait`.
  else if (nevents > 0) {
    __wasi_poll_event_t* received_events = malloc(sizeof(__wasi_poll_event_t) * nevents);
    uint32_t received_events_size = nevents;
    __wasi_errno_t err = poller_wait(kq, received_events, nevents, &received_events_size);

    if (err != 0) {
      return err;
    }

    if ((int)(received_events_size) > nevents) {
      return -1;
    }

    for (uint32_t nth = 0; nth < received_events_size; ++nth) {
      __wasi_poll_event_t* received_event = &received_events[nth];
      struct kevent* event = &eventlist[nth];

      event->ident = received_event->token;

      if (received_event->readable) {
        event->filter = EVFILT_READ;
      } else if (received_event->writable) {
        event->filter = EVFILT_WRITE;
      } else {
        event->filter = 0;
      }

      event->flags = 0;
      event->fflags = 0;
      event->data = 0;
      event->udata = NULL;
    }
  }

  return 0;
}
