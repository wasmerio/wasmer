#define _GNU_SOURCE
#include <node_api.h>
#include <signal.h>
#include <stdint.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>

#if !defined(__linux__) || !defined(__x86_64__)
#error This sampler supports Linux x86-64 only
#endif

static uintptr_t ips[100000];
static volatile sig_atomic_t used, dropped;
static int signo, active;
static struct sigaction previous;
static timer_t timer;

static void capture(int sig, siginfo_t *info, void *context) {
  (void)sig;
  if (info->si_code != SI_TIMER) return;
  if ((size_t)used < sizeof(ips) / sizeof(ips[0])) {
    ips[used++] = (uintptr_t)((ucontext_t *)context)->uc_mcontext.gregs[REG_RIP];
  } else {
    ++dropped;
  }
}

static napi_value fail(napi_env env, const char *why) {
  napi_throw_error(env, NULL, why);
  return NULL;
}

static napi_value start(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], value;
  uint32_t interval;
  if (active || napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok ||
      argc != 1 || napi_get_value_uint32(env, argv[0], &interval) != napi_ok ||
      interval < 100000 || interval > 10000000)
    return fail(env, "Invalid sampling interval or sampler already active");
  sigset_t blocked;
  if (sigprocmask(SIG_SETMASK, NULL, &blocked)) return fail(env, "Read signal mask");
  for (signo = SIGRTMIN + 3; signo <= SIGRTMAX; ++signo) {
    if (!sigismember(&blocked, signo) && !sigaction(signo, NULL, &previous) &&
        previous.sa_handler == SIG_DFL) break;
  }
  if (signo > SIGRTMAX) return fail(env, "No unused real-time signal");
  struct sigaction action = {0};
  action.sa_sigaction = capture;
  action.sa_flags = SA_SIGINFO | SA_RESTART;
  sigemptyset(&action.sa_mask);
  if (sigaction(signo, &action, NULL)) return fail(env, "Install timer handler");
  struct sigevent event = {0};
  event.sigev_notify = SIGEV_THREAD_ID;
  event.sigev_signo = signo;
  event._sigev_un._tid = (int)syscall(SYS_gettid);
  if (timer_create(CLOCK_THREAD_CPUTIME_ID, &event, &timer)) {
    sigaction(signo, &previous, NULL);
    return fail(env, "Create CPU timer for Node main thread");
  }
  struct itimerspec spec = {{0, interval}, {0, interval}};
  used = dropped = 0;
  if (timer_settime(timer, 0, &spec, NULL)) {
    timer_delete(timer);
    sigaction(signo, &previous, NULL);
    return fail(env, "Arm CPU timer");
  }
  active = 1;
  napi_get_undefined(env, &value);
  return value;
}

static napi_value stop(napi_env env, napi_callback_info info) {
  (void)info;
  if (!active) return fail(env, "Sampler inactive");
  sigset_t set, old;
  sigemptyset(&set);
  sigaddset(&set, signo);
  if (sigprocmask(SIG_BLOCK, &set, &old)) return fail(env, "Block sampler signal");
  struct itimerspec off = {{0, 0}, {0, 0}};
  timer_settime(timer, 0, &off, NULL);
  timer_delete(timer);
  struct timespec timeout = {0, 0};
  while (sigtimedwait(&set, NULL, &timeout) >= 0) { }
  sigaction(signo, &previous, NULL);
  sigprocmask(SIG_SETMASK, &old, NULL);
  active = 0;
  if (dropped) return fail(env, "PC buffer overflow");
  napi_value buffer;
  if (napi_create_buffer_copy(env, used * sizeof(ips[0]), ips, NULL, &buffer) != napi_ok)
    return fail(env, "Create PC buffer");
  return buffer;
}

NAPI_MODULE_INIT() {
  napi_property_descriptor properties[] = {
    {"start", NULL, start, NULL, NULL, NULL, napi_default, NULL},
    {"stop", NULL, stop, NULL, NULL, NULL, napi_default, NULL}
  };
  if (napi_define_properties(env, exports, 2, properties) != napi_ok)
    return fail(env, "Register profiler");
  return exports;
}
