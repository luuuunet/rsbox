#ifndef RSBOX_H
#define RSBOX_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Static version string (NUL-terminated, do not free). */
const char *rsbox_version(void);

/** Extended version label for mobile shells. */
const char *rsbox_version_full(void);

/** Parse config JSON; returns 0 on success, -1 on error. */
int32_t rsbox_check_config(const char *config_json);

/** Start from config file path; returns 0 on success. */
int32_t rsbox_start(const char *config_path);

/** Start from in-memory config JSON (preferred on Android/iOS); returns 0 on success. */
int32_t rsbox_start_config(const char *config_json);

/** Stop if running; returns 0 on success. */
int32_t rsbox_stop(void);

#ifdef __cplusplus
}
#endif

#endif /* RSBOX_H */
