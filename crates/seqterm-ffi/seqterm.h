/**
 * seqterm.h — C API for embedding SeqTerm.
 *
 * Build the shared library with:
 *   cargo build --release -p seqterm-ffi
 *
 * Link with -lseqterm_ffi and include this header.
 */
#ifndef SEQTERM_H
#define SEQTERM_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Opaque project handle. */
typedef struct SeqTermProject seqterm_project_t;

/**
 * Open a project from a JSON or .seqterm file.
 * Returns NULL on failure. Free with seqterm_project_free().
 */
seqterm_project_t *seqterm_project_open(const char *path);

/**
 * Create a blank project with the given name.
 * Returns NULL on allocation failure. Free with seqterm_project_free().
 */
seqterm_project_t *seqterm_project_new(const char *name);

/**
 * Save a project to a JSON file.
 * Returns 0 on success, -1 on failure.
 */
int seqterm_project_save(const seqterm_project_t *project, const char *path);

/**
 * Serialize a project to a JSON string.
 * The caller must free the returned string with seqterm_string_free().
 * Returns NULL on failure.
 */
char *seqterm_project_to_json(const seqterm_project_t *project);

/**
 * Get the project name (read-only; valid until project is freed).
 */
const char *seqterm_project_name(const seqterm_project_t *project);

/** Get the project BPM. */
double seqterm_project_get_bpm(const seqterm_project_t *project);

/** Set the project BPM. */
void seqterm_project_set_bpm(seqterm_project_t *project, double bpm);

/** Return the number of mixer channels in the project. */
int seqterm_project_channel_count(const seqterm_project_t *project);

/**
 * Return the SeqTerm SDK version string (static; no need to free).
 */
const char *seqterm_sdk_version(void);

/**
 * Free a project obtained from seqterm_project_new() or seqterm_project_open().
 * Passing NULL is a no-op.
 */
void seqterm_project_free(seqterm_project_t *project);

/**
 * Free a string returned by a SeqTerm function (e.g. seqterm_project_to_json()).
 * Passing NULL is a no-op.
 */
void seqterm_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* SEQTERM_H */
