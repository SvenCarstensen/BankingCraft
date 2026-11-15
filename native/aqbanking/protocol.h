/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef BANKINGCRAFT_PROTOCOL_H
#define BANKINGCRAFT_PROTOCOL_H
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Private, versioned pipe protocol. No command line secrets or temporary data
 * files. Each frame is a bounded map: u32 length, then (u16 key length, key,
 * u32 value length, value)*. All lengths are big endian. */
#define BC_FRAME_LIMIT (32u * 1024u * 1024u)
#define BC_FIELD_LIMIT 128u
typedef struct { char *key; unsigned char *value; uint32_t size; } BC_FIELD;
typedef struct { BC_FIELD fields[BC_FIELD_LIMIT]; unsigned count; } BC_FRAME;

static FILE *bc_output;
static void bc_wipe(void *ptr, size_t n) { volatile unsigned char *p=ptr; while (n--) *p++=0; }
static void bc_free(BC_FRAME *f) {
  for (unsigned i=0; i<f->count; ++i) {
    free(f->fields[i].key);
    bc_wipe(f->fields[i].value, f->fields[i].size);
    free(f->fields[i].value);
  }
  memset(f, 0, sizeof(*f));
}
static uint32_t bc_u32(const unsigned char *p) {
  return ((uint32_t)p[0]<<24)|((uint32_t)p[1]<<16)|((uint32_t)p[2]<<8)|p[3];
}
static void bc_put32(unsigned char *p, uint32_t n) {
  p[0]=n>>24; p[1]=n>>16; p[2]=n>>8; p[3]=n;
}
static const BC_FIELD *bc_field(const BC_FRAME *f, const char *key) {
  for (unsigned i=0; i<f->count; ++i) if (!strcmp(f->fields[i].key,key)) return &f->fields[i];
  return NULL;
}
static const char *bc_text(const BC_FRAME *f, const char *key) {
  const BC_FIELD *v=bc_field(f,key);
  if (!v) return "";
  /* Binary values must be accessed with bc_field, never as strings. */
  if (memchr(v->value, 0, v->size)) exit(71);
  return (const char *)v->value;
}
static void bc_add(BC_FRAME *f, const char *key, const void *value, size_t size) {
  if (f->count>=BC_FIELD_LIMIT || strlen(key)>255 || size>BC_FRAME_LIMIT || bc_field(f,key)) exit(71);
  BC_FIELD *v=&f->fields[f->count++];
  v->key=strdup(key); v->value=calloc(size+1,1); v->size=(uint32_t)size;
  if (!v->key || !v->value) exit(71);
  if (size) memcpy(v->value,value,size);
}
static void bc_string(BC_FRAME *f, const char *key, const char *value) {
  bc_add(f,key,value?value:"",value?strlen(value):0);
}
static void bc_number(BC_FRAME *f,const char *key,long value) {
  char b[32]; snprintf(b,sizeof(b),"%ld",value); bc_string(f,key,b);
}
static int bc_read(BC_FRAME *f) {
  unsigned char h[4];
  size_t got=fread(h,1,4,stdin);
  if (!got && feof(stdin)) return 0;
  if (got!=4) exit(71);
  uint32_t size=bc_u32(h);
  if (size>BC_FRAME_LIMIT) exit(71);
  unsigned char *data=malloc(size?size:1);
  if (!data || fread(data,1,size,stdin)!=size) exit(71);
  size_t p=0;
  while (p<size) {
    if (size-p<2) exit(71);
    unsigned k=((unsigned)data[p]<<8)|data[p+1]; p+=2;
    if (!k || k>255 || size-p<k+4 || memchr(data+p,0,k)) exit(71);
    char key[256]; memcpy(key,data+p,k); key[k]=0; p+=k;
    uint32_t n=bc_u32(data+p); p+=4;
    if (n>size-p) exit(71);
    bc_add(f,key,data+p,n); p+=n;
  }
  bc_wipe(data,size); free(data); return 1;
}
static void bc_send(BC_FRAME *f) {
  size_t size=0;
  for(unsigned i=0;i<f->count;i++) size+=6+strlen(f->fields[i].key)+f->fields[i].size;
  if(size>BC_FRAME_LIMIT) exit(71);
  unsigned char h[4]; bc_put32(h,(uint32_t)size);
  if(fwrite(h,1,4,bc_output)!=4) exit(71);
  for(unsigned i=0;i<f->count;i++) {
    BC_FIELD *v=&f->fields[i]; size_t k=strlen(v->key);
    unsigned char kh[2]={k>>8,k};
    if(fwrite(kh,1,2,bc_output)!=2 || fwrite(v->key,1,k,bc_output)!=k) exit(71);
    bc_put32(h,v->size);
    if(fwrite(h,1,4,bc_output)!=4 || fwrite(v->value,1,v->size,bc_output)!=v->size) exit(71);
  }
  if(fflush(bc_output)) exit(71);
  bc_free(f);
}
#endif
