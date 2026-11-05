# CPU from launch to the first frame, by purpose and mechanism

From perf record (cpu-clock:u, one sample per 100 us of CPU, DWARF call chains), 3 runs each, samples cut at sway's window event; made by attribute.py. User time only; the runs are 15-55% slower than clean ones.

<a id="kit"></a>

## kit: 3 runs, first frame (under perf) median 29.7 ms
CPU to first frame (schedstat, all threads) median 27.9 ms: user (sampled) 15.7 ms, kernel and unsampled 12.2 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| fontconfig | 14.23 | 88.2% |
| other | 1.50 | 9.3% |
| exec and dynamic linking (before main) | 0.13 | 0.8% |
| app and toolkit start | 0.13 | 0.8% |
| layout and drawing | 0.07 | 0.4% |
| fonts: shaping and glyphs | 0.03 | 0.2% |
| dlopen (other) | 0.03 | 0.2% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| fontconfig, FreeType, HarfBuzz | 10.73 | 66.5% |
| C library (malloc, strings, I/O) | 2.73 | 16.9% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.33 | 8.3% |
| the program itself | 0.60 | 3.7% |
| other | 0.47 | 2.9% |
| dynamic linker | 0.27 | 1.7% |

Largest purpose x mechanism cells (ms per run):
    10.73  fontconfig  /  fontconfig, FreeType, HarfBuzz
     2.30  fontconfig  /  C library (malloc, strings, I/O)
     1.20  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.47  other  /  the program itself
     0.47  other  /  other
     0.43  other  /  C library (malloc, strings, I/O)
     0.13  exec and dynamic linking (before main)  /  dynamic linker
     0.13  other  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.10  app and toolkit start  /  dynamic linker
     0.07  layout and drawing  /  the program itself
     0.03  fonts: shaping and glyphs  /  the program itself
     0.03  app and toolkit start  /  the program itself
     0.03  dlopen (other)  /  dynamic linker

Threads (ms per run): hello-kit 15.8, sidestep-render 0.3

Leaf libraries (self ms per run):
    10.73  libfontconfig.so.1.16.1
     2.73  libc.so.6
     1.33  libexpat.so.1.13.0
     0.60  hello-kit
     0.27  ld-linux-x86-64.so.2

Leaf functions (self ms per run):
     1.87  FcCharSetIsSubset  (libfontconfig.so.1.16.1)
     1.43  FcSortCompare  (libfontconfig.so.1.16.1)
     0.70  FcCharSetFindLeafForward  (libfontconfig.so.1.16.1)
     0.70  FcStrHashIgnoreBlanksAndCase  (libfontconfig.so.1.16.1)
     0.57  FcCompare  (libfontconfig.so.1.16.1)
     0.50  FcPatternObjectPosition  (libfontconfig.so.1.16.1)
     0.43  FcHashTableDestroy  (libfontconfig.so.1.16.1)
     0.40  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.40  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.33  FcValueCanonicalize  (libfontconfig.so.1.16.1)
     0.30  _int_malloc  (libc.so.6)
     0.27  normal_contentTok  (libexpat.so.1.13.0)
     0.27  msort_with_tmp.part.0  (libc.so.6)
     0.23  cfree@GLIBC_2.2.5  (libc.so.6)
     0.23  __memset_avx512_unaligned_erms  (libc.so.6)
     0.23  FcCompareEqual  (libfontconfig.so.1.16.1)
     0.20  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.20  FcPatternObjectListAdd  (libfontconfig.so.1.16.1)
     0.17  FcCompareValueList  (libfontconfig.so.1.16.1)
     0.17  FcConfigEvaluate  (libfontconfig.so.1.16.1)
     0.17  IA__FcConfigSubstituteWithPat.part.0  (libfontconfig.so.1.16.1)
     0.17  _int_free_create_chunk  (libc.so.6)
     0.13  normal_updatePosition  (libexpat.so.1.13.0)
     0.13  FcObjectLookupIdByName  (libfontconfig.so.1.16.1)
     0.13  FcFontSetSort  (libfontconfig.so.1.16.1)
     0.13  FcCharSetMerge  (libfontconfig.so.1.16.1)
     0.13  FcStrHashIgnoreCase  (libfontconfig.so.1.16.1)
     0.13  FcHashTableFind  (libfontconfig.so.1.16.1)
     0.13  FcValueListDuplicate  (libfontconfig.so.1.16.1)
     0.13  FcStrCmpIgnoreCase  (libfontconfig.so.1.16.1)


<a id="kit.dejavu-only"></a>

## kit.dejavu-only: 3 runs, first frame (under perf) median 15.2 ms
CPU to first frame (schedstat, all threads) median 14.9 ms: user (sampled) 3.9 ms, kernel and unsampled 11.0 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| fontconfig | 2.80 | 74.3% |
| other | 0.73 | 19.5% |
| app and toolkit start | 0.17 | 4.4% |
| exec and dynamic linking (before main) | 0.03 | 0.9% |
| Wayland and keymaps | 0.03 | 0.9% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| C library (malloc, strings, I/O) | 1.20 | 31.9% |
| fontconfig, FreeType, HarfBuzz | 0.97 | 25.7% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 0.80 | 21.2% |
| the program itself | 0.40 | 10.6% |
| other | 0.23 | 6.2% |
| dynamic linker | 0.17 | 4.4% |

Largest purpose x mechanism cells (ms per run):
     1.03  fontconfig  /  C library (malloc, strings, I/O)
     0.97  fontconfig  /  fontconfig, FreeType, HarfBuzz
     0.80  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.37  other  /  the program itself
     0.23  other  /  other
     0.13  other  /  C library (malloc, strings, I/O)
     0.13  app and toolkit start  /  dynamic linker
     0.03  app and toolkit start  /  the program itself
     0.03  exec and dynamic linking (before main)  /  dynamic linker
     0.03  Wayland and keymaps  /  C library (malloc, strings, I/O)

Threads (ms per run): hello-kit 3.4, sidestep-render 0.3, sidestep-compos 0.0

Leaf libraries (self ms per run):
     1.20  libc.so.6
     0.97  libfontconfig.so.1.16.1
     0.80  libexpat.so.1.13.0
     0.40  hello-kit
     0.17  ld-linux-x86-64.so.2

Leaf functions (self ms per run):
     0.23  __strlen_evex512  (libc.so.6)
     0.17  __strcmp_evex  (libc.so.6)
     0.17  _int_malloc  (libc.so.6)
     0.17  FcStrSetMember  (libfontconfig.so.1.16.1)
     0.13  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.13  normal_contentTok  (libexpat.so.1.13.0)
     0.10  cfree@GLIBC_2.2.5  (libc.so.6)
     0.10  FcStrBufChar  (libfontconfig.so.1.16.1)
     0.10  normal_updatePosition  (libexpat.so.1.13.0)
     0.10  FcStrHashIgnoreBlanksAndCase  (libfontconfig.so.1.16.1)
     0.10  FcPtrListIterIsValid  (libfontconfig.so.1.16.1)
     0.07  sip24_final  (libexpat.so.1.13.0)
     0.07  malloc  (libc.so.6)
     0.07  __libc_malloc2  (libc.so.6)
     0.07  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     0.07  __strncmp_evex  (libc.so.6)
     0.07  _int_free_chunk  (libc.so.6)
     0.07  normal_getAtts  (libexpat.so.1.13.0)
     0.03  sysmalloc  (libc.so.6)
     0.03  call_init  (ld-linux-x86-64.so.2)
     0.03  doctype4  (libexpat.so.1.13.0)
     0.03  FcStartElement  (libfontconfig.so.1.16.1)
     0.03  normal_scanComment.part.0  (libexpat.so.1.13.0)
     0.03  doContent  (libexpat.so.1.13.0)
     0.03  FcStrBufData  (libfontconfig.so.1.16.1)
     0.03  lookupWithLength  (libexpat.so.1.13.0)
     0.03  utf8_toUtf8  (libexpat.so.1.13.0)
     0.03  __GI___pthread_mutex_unlock_usercnt  (libc.so.6)
     0.03  copy_string  (libfontconfig.so.1.16.1)
     0.03  FcPatternObjectPosition  (libfontconfig.so.1.16.1)


<a id="sumi"></a>

## sumi: 3 runs, first frame (under perf) median 27.9 ms
CPU to first frame (schedstat, all threads) median 27.5 ms: user (sampled) 15.2 ms, kernel and unsampled 12.2 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| fontconfig | 13.60 | 89.1% |
| layout and drawing | 0.67 | 4.4% |
| other | 0.67 | 4.4% |
| Wayland and keymaps | 0.13 | 0.9% |
| app and toolkit start | 0.13 | 0.9% |
| exec and dynamic linking (before main) | 0.07 | 0.4% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| fontconfig, FreeType, HarfBuzz | 9.33 | 61.1% |
| C library (malloc, strings, I/O) | 3.97 | 26.0% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 0.97 | 6.3% |
| the program itself | 0.87 | 5.7% |
| dynamic linker | 0.10 | 0.7% |
| other | 0.03 | 0.2% |

Largest purpose x mechanism cells (ms per run):
     9.33  fontconfig  /  fontconfig, FreeType, HarfBuzz
     3.50  fontconfig  /  C library (malloc, strings, I/O)
     0.77  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.40  layout and drawing  /  the program itself
     0.27  layout and drawing  /  C library (malloc, strings, I/O)
     0.27  other  /  the program itself
     0.20  other  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.17  other  /  C library (malloc, strings, I/O)
     0.13  Wayland and keymaps  /  the program itself
     0.07  exec and dynamic linking (before main)  /  dynamic linker
     0.07  app and toolkit start  /  the program itself
     0.03  app and toolkit start  /  C library (malloc, strings, I/O)
     0.03  app and toolkit start  /  dynamic linker
     0.03  other  /  other

Threads (ms per run): aizome-sumi 15.1, sidestep-render 0.2

Leaf libraries (self ms per run):
     9.33  libfontconfig.so.1.16.1
     3.97  libc.so.6
     0.97  libexpat.so.1.13.0
     0.87  aizome-sumi
     0.10  ld-linux-x86-64.so.2

Leaf functions (self ms per run):
     1.90  FcCharSetIsSubset  (libfontconfig.so.1.16.1)
     1.50  FcSortCompare  (libfontconfig.so.1.16.1)
     0.70  FcCharSetFindLeafForward  (libfontconfig.so.1.16.1)
     0.67  FcCompare  (libfontconfig.so.1.16.1)
     0.53  FcPatternObjectPosition  (libfontconfig.so.1.16.1)
     0.53  FcStrHashIgnoreBlanksAndCase  (libfontconfig.so.1.16.1)
     0.47  __libc_malloc2  (libc.so.6)
     0.43  _int_malloc  (libc.so.6)
     0.43  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.33  <fontique::family_name::NameKey>::from_str  (aizome-sumi)
     0.30  __strlen_evex512  (libc.so.6)
     0.30  FcHashTableDestroy  (libfontconfig.so.1.16.1)
     0.27  normal_updatePosition  (libexpat.so.1.13.0)
     0.27  _int_free_chunk  (libc.so.6)
     0.23  malloc  (libc.so.6)
     0.23  FcStrHashIgnoreCase  (libfontconfig.so.1.16.1)
     0.23  _int_free_create_chunk  (libc.so.6)
     0.23  msort_with_tmp.part.0  (libc.so.6)
     0.23  __libc_calloc2  (libc.so.6)
     0.23  FcHashTableFind  (libfontconfig.so.1.16.1)
     0.23  FcPatternObjectInsertElt  (libfontconfig.so.1.16.1)
     0.20  __strcmp_evex  (libc.so.6)
     0.17  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.17  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.17  cfree@GLIBC_2.2.5  (libc.so.6)
     0.17  getenv  (libc.so.6)
     0.13  normal_contentTok  (libexpat.so.1.13.0)
     0.13  FcStrBufChar  (libfontconfig.so.1.16.1)
     0.13  FcFontSetSort  (libfontconfig.so.1.16.1)
     0.13  FcValueCanonicalize  (libfontconfig.so.1.16.1)


<a id="sumi.dejavu-only"></a>

## sumi.dejavu-only: 3 runs, first frame (under perf) median 14.8 ms
CPU to first frame (schedstat, all threads) median 14.5 ms: user (sampled) 3.2 ms, kernel and unsampled 11.4 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| fontconfig | 2.43 | 75.3% |
| other | 0.30 | 9.3% |
| exec and dynamic linking (before main) | 0.20 | 6.2% |
| app and toolkit start | 0.20 | 6.2% |
| Wayland and keymaps | 0.07 | 2.1% |
| layout and drawing | 0.03 | 1.0% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| fontconfig, FreeType, HarfBuzz | 0.97 | 29.9% |
| C library (malloc, strings, I/O) | 0.93 | 28.9% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 0.70 | 21.6% |
| the program itself | 0.33 | 10.3% |
| dynamic linker | 0.30 | 9.3% |

Largest purpose x mechanism cells (ms per run):
     0.97  fontconfig  /  fontconfig, FreeType, HarfBuzz
     0.80  fontconfig  /  C library (malloc, strings, I/O)
     0.67  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.20  exec and dynamic linking (before main)  /  dynamic linker
     0.17  other  /  the program itself
     0.10  other  /  C library (malloc, strings, I/O)
     0.10  app and toolkit start  /  the program itself
     0.07  app and toolkit start  /  dynamic linker
     0.07  Wayland and keymaps  /  the program itself
     0.03  app and toolkit start  /  C library (malloc, strings, I/O)
     0.03  other  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.03  layout and drawing  /  dynamic linker

Threads (ms per run): aizome-sumi 3.1, sidestep-render 0.1

Leaf libraries (self ms per run):
     0.97  libfontconfig.so.1.16.1
     0.93  libc.so.6
     0.70  libexpat.so.1.13.0
     0.33  aizome-sumi
     0.30  ld-linux-x86-64.so.2

Leaf functions (self ms per run):
     0.23  __libc_malloc2  (libc.so.6)
     0.20  FcStrBufChar  (libfontconfig.so.1.16.1)
     0.13  normal_updatePosition  (libexpat.so.1.13.0)
     0.13  FcStrHashIgnoreBlanksAndCase  (libfontconfig.so.1.16.1)
     0.10  normal_contentTok  (libexpat.so.1.13.0)
     0.10  __strlen_evex512  (libc.so.6)
     0.07  __strcmp_evex  (libc.so.6)
     0.07  FcStrSetMember  (libfontconfig.so.1.16.1)
     0.07  doContent  (libexpat.so.1.13.0)
     0.07  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.07  _int_malloc  (libc.so.6)
     0.07  __libc_calloc2  (libc.so.6)
     0.07  FcPtrListIterIsValid  (libfontconfig.so.1.16.1)
     0.07  check_match  (ld-linux-x86-64.so.2)
     0.07  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.07  malloc  (libc.so.6)
     0.07  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     0.07  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     0.03  do_lookup_x  (ld-linux-x86-64.so.2)
     0.03  normal_scanComment.part.0  (libexpat.so.1.13.0)
     0.03  FcEndElement  (libfontconfig.so.1.16.1)
     0.03  FcObjectValidType  (libfontconfig.so.1.16.1)
     0.03  IA__FcPatternDestroy.part.0  (libfontconfig.so.1.16.1)
     0.03  msort_with_tmp.part.0  (libc.so.6)
     0.03  FcFontRenderPrepare  (libfontconfig.so.1.16.1)
     0.03  FcPtrListIterInit  (libfontconfig.so.1.16.1)
     0.03  _int_realloc  (libc.so.6)
     0.03  core::str::converts::from_utf8  (aizome-sumi)
     0.03  <ropey::tree::text_info::TextInfo>::from_str  (aizome-sumi)
     0.03  <wayland_client::protocol::wl_surface::WlSurface>::set_input_region  (aizome-sumi)


<a id="gtk4"></a>

## gtk4: 3 runs, first frame (under perf) median 113.5 ms
CPU to first frame (schedstat, all threads) median 115.9 ms: user (sampled) 67.7 ms, kernel and unsampled 48.3 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| GPU: Vulkan instance and devices | 29.47 | 43.4% |
| GPU: driver code (other) | 8.50 | 12.5% |
| exec and dynamic linking (before main) | 8.00 | 11.8% |
| CSS, theme and style | 6.30 | 9.3% |
| fontconfig | 5.27 | 7.7% |
| icon theme and images | 1.77 | 2.6% |
| Wayland and keymaps | 1.67 | 2.5% |
| other | 1.27 | 1.9% |
| fonts: shaping and glyphs | 1.20 | 1.8% |
| D-Bus, portals and accessibility | 1.00 | 1.5% |
| GObject type and class setup | 0.97 | 1.4% |
| GPU: EGL and GL context | 0.90 | 1.3% |
| GIO and GTK modules | 0.73 | 1.1% |
| app and toolkit start | 0.37 | 0.5% |
| library constructors (before main) | 0.27 | 0.4% |
| layout and drawing | 0.17 | 0.2% |
| GPU: rendering the frame | 0.13 | 0.2% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 24.57 | 36.1% |
| GPU drivers (Mesa, LLVM, NVIDIA) | 12.33 | 18.1% |
| C library (malloc, strings, I/O) | 11.67 | 17.2% |
| GLib and GObject | 5.17 | 7.6% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 5.03 | 7.4% |
| GTK and libadwaita | 4.33 | 6.4% |
| fontconfig, FreeType, HarfBuzz | 3.50 | 5.1% |
| Wayland and xkbcommon | 1.03 | 1.5% |
| other | 0.33 | 0.5% |

Largest purpose x mechanism cells (ms per run):
    14.90  GPU: Vulkan instance and devices  /  dynamic linker
    10.23  GPU: Vulkan instance and devices  /  GPU drivers (Mesa, LLVM, NVIDIA)
     8.00  exec and dynamic linking (before main)  /  dynamic linker
     4.27  GPU: Vulkan instance and devices  /  C library (malloc, strings, I/O)
     3.73  CSS, theme and style  /  GTK and libadwaita
     3.67  GPU: driver code (other)  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     2.67  GPU: driver code (other)  /  C library (malloc, strings, I/O)
     2.60  fontconfig  /  fontconfig, FreeType, HarfBuzz
     2.07  GPU: driver code (other)  /  GPU drivers (Mesa, LLVM, NVIDIA)
     1.83  CSS, theme and style  /  GLib and GObject
     1.50  fontconfig  /  C library (malloc, strings, I/O)
     1.17  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     1.00  Wayland and keymaps  /  Wayland and xkbcommon
     0.90  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     0.87  GPU: EGL and GL context  /  dynamic linker

Threads (ms per run): .hello-gtk4-wra 63.7, [pango] fontcon 3.8, gdbus 0.5, hello-gtk4 0.0

Leaf libraries (self ms per run):
    24.57  ld-linux-x86-64.so.2
    11.63  libc.so.6
     5.73  libnvidia-glcore.so.615.78.08
     5.03  libexpat.so.1.13.0
     4.50  libnvidia-gpucomp.so.615.78.08
     4.30  libgtk-4.so.1.2200.4
     4.07  libglib-2.0.so.0.8800.3
     2.60  libfontconfig.so.1.16.1
     0.83  libxkbcommon.so.0.13.2
     0.73  libvulkan_radeon.so
     0.67  libgobject-2.0.so.0.8800.3
     0.60  libfreetype.so.6.20.6
     0.57  libLLVM.so.21.1
     0.43  libgio-2.0.so.0.8800.3
     0.17  libvulkan.so.1.4.363

Leaf functions (self ms per run):
    14.30  do_lookup_x  (ld-linux-x86-64.so.2)
     5.73  [unknown]  (libnvidia-glcore.so.615.78.08)
     4.43  [unknown]  (libnvidia-gpucomp.so.615.78.08)
     3.73  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     2.07  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     2.03  strcmp  (ld-linux-x86-64.so.2)
     1.87  __strcmp_evex  (libc.so.6)
     1.13  normal_updatePosition  (libexpat.so.1.13.0)
     0.97  check_match  (ld-linux-x86-64.so.2)
     0.90  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     0.83  normal_contentTok  (libexpat.so.1.13.0)
     0.77  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.73  __libc_malloc2  (libc.so.6)
     0.70  __strlen_evex512  (libc.so.6)
     0.70  __GI_____strtoull_l_internal  (libc.so.6)
     0.67  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.57  subdivide_infos  (libgtk-4.so.1.2200.4)
     0.50  gtk_css_tokenizer_read_name  (libgtk-4.so.1.2200.4)
     0.47  __strncpy_evex  (libc.so.6)
     0.43  realloc  (libc.so.6)
     0.40  normal_getAtts  (libexpat.so.1.13.0)
     0.37  _int_free_chunk  (libc.so.6)
     0.37  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.33  FcValueCanonicalize  (libfontconfig.so.1.16.1)
     0.30  __strncmp_evex  (libc.so.6)
     0.30  _xkbcommon_parse  (libxkbcommon.so.0.13.2)
     0.30  gtk_css_parser_ensure_token  (libgtk-4.so.1.2200.4)
     0.30  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     0.30  cfree@GLIBC_2.2.5  (libc.so.6)
     0.27  _gtk_css_selector_compare  (libgtk-4.so.1.2200.4)


<a id="gtk4.lean"></a>

## gtk4.lean: 3 runs, first frame (under perf) median 37.7 ms
CPU to first frame (schedstat, all threads) median 41.6 ms: user (sampled) 25.6 ms, kernel and unsampled 16.0 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| exec and dynamic linking (before main) | 8.03 | 31.1% |
| CSS, theme and style | 6.30 | 24.4% |
| fontconfig | 4.83 | 18.7% |
| icon theme and images | 1.93 | 7.5% |
| Wayland and keymaps | 1.20 | 4.7% |
| fonts: shaping and glyphs | 0.93 | 3.6% |
| GObject type and class setup | 0.70 | 2.7% |
| other | 0.57 | 2.2% |
| GIO and GTK modules | 0.33 | 1.3% |
| layout and drawing | 0.33 | 1.3% |
| library constructors (before main) | 0.23 | 0.9% |
| app and toolkit start | 0.20 | 0.8% |
| D-Bus, portals and accessibility | 0.20 | 0.8% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 8.43 | 32.7% |
| GLib and GObject | 4.70 | 18.2% |
| C library (malloc, strings, I/O) | 4.37 | 16.9% |
| GTK and libadwaita | 3.53 | 13.7% |
| fontconfig, FreeType, HarfBuzz | 2.90 | 11.2% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.17 | 4.5% |
| Wayland and xkbcommon | 0.63 | 2.5% |
| other | 0.07 | 0.3% |

Largest purpose x mechanism cells (ms per run):
     7.97  exec and dynamic linking (before main)  /  dynamic linker
     3.13  CSS, theme and style  /  GTK and libadwaita
     2.33  fontconfig  /  fontconfig, FreeType, HarfBuzz
     2.20  CSS, theme and style  /  GLib and GObject
     1.43  fontconfig  /  C library (malloc, strings, I/O)
     1.07  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.97  CSS, theme and style  /  C library (malloc, strings, I/O)
     0.93  icon theme and images  /  GLib and GObject
     0.90  icon theme and images  /  C library (malloc, strings, I/O)
     0.63  Wayland and keymaps  /  Wayland and xkbcommon
     0.57  GObject type and class setup  /  GLib and GObject
     0.57  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     0.43  Wayland and keymaps  /  C library (malloc, strings, I/O)
     0.33  GIO and GTK modules  /  dynamic linker
     0.30  other  /  GLib and GObject

Threads (ms per run): .hello-gtk4-wra 21.9, [pango] fontcon 3.8, hello-gtk4 0.1

Leaf libraries (self ms per run):
     8.43  ld-linux-x86-64.so.2
     4.33  libc.so.6
     3.73  libglib-2.0.so.0.8800.3
     3.40  libgtk-4.so.1.2200.4
     2.33  libfontconfig.so.1.16.1
     1.17  libexpat.so.1.13.0
     0.63  libgobject-2.0.so.0.8800.3
     0.53  libxkbcommon.so.0.13.2
     0.40  libfreetype.so.6.20.6
     0.33  libgio-2.0.so.0.8800.3
     0.17  libharfbuzz.so.0.61451.0
     0.10  libwayland-client.so.0.26.0
     0.10  libcairo.so.2.11804.4
     0.07  libtasn1.so.6.6.5
     0.03  libstdc++.so.6.0.36

Leaf functions (self ms per run):
     5.17  do_lookup_x  (ld-linux-x86-64.so.2)
     1.07  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     1.07  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     0.90  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     0.63  strcmp  (ld-linux-x86-64.so.2)
     0.60  __libc_malloc2  (libc.so.6)
     0.53  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     0.43  __strcmp_evex  (libc.so.6)
     0.43  g_hash_table_resize  (libglib-2.0.so.0.8800.3)
     0.40  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.40  gtk_css_parser_ensure_token  (libgtk-4.so.1.2200.4)
     0.40  subdivide_infos  (libgtk-4.so.1.2200.4)
     0.40  _int_malloc  (libc.so.6)
     0.33  _int_free_chunk  (libc.so.6)
     0.33  malloc  (libc.so.6)
     0.30  check_match  (ld-linux-x86-64.so.2)
     0.30  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.27  realloc  (libc.so.6)
     0.27  _gtk_css_selector_compare  (libgtk-4.so.1.2200.4)
     0.27  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.23  normal_contentTok  (libexpat.so.1.13.0)
     0.23  gtk_css_tokenizer_read_name  (libgtk-4.so.1.2200.4)
     0.23  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.23  FcCompareValueList  (libfontconfig.so.1.16.1)
     0.20  normal_updatePosition  (libexpat.so.1.13.0)
     0.20  gtk_css_tokenizer_read_token  (libgtk-4.so.1.2200.4)
     0.20  gtk_css_selector_hash_one  (libgtk-4.so.1.2200.4)
     0.20  gvdb_table_lookup  (libgio-2.0.so.0.8800.3)
     0.20  g_str_hash  (libglib-2.0.so.0.8800.3)
     0.17  g_mutex_lock  (libglib-2.0.so.0.8800.3)


<a id="adw"></a>

## adw: 3 runs, first frame (under perf) median 137.2 ms
CPU to first frame (schedstat, all threads) median 135.3 ms: user (sampled) 88.3 ms, kernel and unsampled 48.3 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| GPU: Vulkan instance and devices | 30.83 | 34.5% |
| CSS, theme and style | 21.03 | 23.5% |
| exec and dynamic linking (before main) | 10.90 | 12.2% |
| GPU: driver code (other) | 7.50 | 8.4% |
| fontconfig | 5.03 | 5.6% |
| other | 3.47 | 3.9% |
| icon theme and images | 1.87 | 2.1% |
| fonts: shaping and glyphs | 1.63 | 1.8% |
| Wayland and keymaps | 1.43 | 1.6% |
| D-Bus, portals and accessibility | 1.27 | 1.4% |
| GPU: EGL and GL context | 1.20 | 1.3% |
| GObject type and class setup | 1.17 | 1.3% |
| GIO and GTK modules | 0.70 | 0.8% |
| app and toolkit start | 0.47 | 0.5% |
| layout and drawing | 0.43 | 0.5% |
| library constructors (before main) | 0.20 | 0.2% |
| GSettings and dconf | 0.10 | 0.1% |
| GPU: pipelines and shaders | 0.07 | 0.1% |
| GPU: rendering the frame | 0.03 | 0.0% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 28.43 | 31.8% |
| C library (malloc, strings, I/O) | 15.33 | 17.2% |
| GTK and libadwaita | 13.00 | 14.6% |
| GPU drivers (Mesa, LLVM, NVIDIA) | 12.23 | 13.7% |
| GLib and GObject | 10.07 | 11.3% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 4.77 | 5.3% |
| fontconfig, FreeType, HarfBuzz | 3.97 | 4.4% |
| other | 0.87 | 1.0% |
| Wayland and xkbcommon | 0.67 | 0.7% |

Largest purpose x mechanism cells (ms per run):
    15.53  GPU: Vulkan instance and devices  /  dynamic linker
    11.93  CSS, theme and style  /  GTK and libadwaita
    10.87  exec and dynamic linking (before main)  /  dynamic linker
    10.60  GPU: Vulkan instance and devices  /  GPU drivers (Mesa, LLVM, NVIDIA)
     5.80  CSS, theme and style  /  GLib and GObject
     4.67  GPU: Vulkan instance and devices  /  C library (malloc, strings, I/O)
     3.30  CSS, theme and style  /  C library (malloc, strings, I/O)
     3.30  GPU: driver code (other)  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     2.63  fontconfig  /  fontconfig, FreeType, HarfBuzz
     2.60  GPU: driver code (other)  /  C library (malloc, strings, I/O)
     1.50  GPU: driver code (other)  /  GPU drivers (Mesa, LLVM, NVIDIA)
     1.47  fontconfig  /  C library (malloc, strings, I/O)
     1.33  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     1.20  GPU: EGL and GL context  /  dynamic linker
     1.10  other  /  GLib and GObject

Threads (ms per run): .hello-adw-wrap 84.9, [pango] fontcon 3.6, gdbus 0.7, hello-adw 0.1

Leaf libraries (self ms per run):
    28.43  ld-linux-x86-64.so.2
    15.27  libc.so.6
    12.77  libgtk-4.so.1.2200.4
     8.23  libglib-2.0.so.0.8800.3
     5.60  libnvidia-glcore.so.615.78.08
     4.90  libnvidia-gpucomp.so.615.78.08
     4.77  libexpat.so.1.13.0
     2.63  libfontconfig.so.1.16.1
     1.17  libgobject-2.0.so.0.8800.3
     0.67  libgio-2.0.so.0.8800.3
     0.57  libxkbcommon.so.0.13.2
     0.53  libLLVM.so.21.1
     0.53  libfreetype.so.6.20.6
     0.40  libharfbuzz.so.0.61451.0
     0.27  libvulkan.so.1.4.363

Leaf functions (self ms per run):
    17.00  do_lookup_x  (ld-linux-x86-64.so.2)
     5.60  [unknown]  (libnvidia-glcore.so.615.78.08)
     4.87  [unknown]  (libnvidia-gpucomp.so.615.78.08)
     4.10  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     2.73  strcmp  (ld-linux-x86-64.so.2)
     2.60  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     2.07  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     1.83  subdivide_infos  (libgtk-4.so.1.2200.4)
     1.53  _int_malloc  (libc.so.6)
     1.47  __strcmp_evex  (libc.so.6)
     1.40  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     1.27  gtk_css_tokenizer_read_name  (libgtk-4.so.1.2200.4)
     1.23  __GI_____strtoull_l_internal  (libc.so.6)
     1.10  gtk_css_parser_ensure_token  (libgtk-4.so.1.2200.4)
     1.10  _gtk_css_selector_compare  (libgtk-4.so.1.2200.4)
     1.03  check_match  (ld-linux-x86-64.so.2)
     1.03  __libc_malloc2  (libc.so.6)
     1.03  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.97  __strlen_evex512  (libc.so.6)
     0.93  normal_updatePosition  (libexpat.so.1.13.0)
     0.83  normal_contentTok  (libexpat.so.1.13.0)
     0.83  realloc  (libc.so.6)
     0.67  gtk_css_selector_equal  (libgtk-4.so.1.2200.4)
     0.63  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.60  gtk_css_tokenizer_read_token  (libgtk-4.so.1.2200.4)
     0.57  normal_getAtts  (libexpat.so.1.13.0)
     0.53  _int_free_chunk  (libc.so.6)
     0.53  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.50  gtk_css_selector_class_add_specificity  (libgtk-4.so.1.2200.4)
     0.50  g_hash_table_resize  (libglib-2.0.so.0.8800.3)


<a id="adw.lean"></a>

## adw.lean: 3 runs, first frame (under perf) median 71.6 ms
CPU to first frame (schedstat, all threads) median 68.1 ms: user (sampled) 49.6 ms, kernel and unsampled 18.2 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| CSS, theme and style | 21.33 | 42.0% |
| exec and dynamic linking (before main) | 11.13 | 21.9% |
| fontconfig | 5.77 | 11.4% |
| icon theme and images | 4.23 | 8.3% |
| other | 1.73 | 3.4% |
| fonts: shaping and glyphs | 1.67 | 3.3% |
| Wayland and keymaps | 1.17 | 2.3% |
| layout and drawing | 1.07 | 2.1% |
| GObject type and class setup | 0.93 | 1.8% |
| app and toolkit start | 0.63 | 1.2% |
| GIO and GTK modules | 0.47 | 0.9% |
| library constructors (before main) | 0.33 | 0.7% |
| D-Bus, portals and accessibility | 0.23 | 0.5% |
| GSettings and dconf | 0.03 | 0.1% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| GTK and libadwaita | 14.53 | 28.6% |
| dynamic linker | 11.70 | 23.1% |
| GLib and GObject | 10.40 | 20.5% |
| C library (malloc, strings, I/O) | 8.10 | 16.0% |
| fontconfig, FreeType, HarfBuzz | 4.13 | 8.1% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.07 | 2.1% |
| Wayland and xkbcommon | 0.70 | 1.4% |
| other | 0.10 | 0.2% |

Largest purpose x mechanism cells (ms per run):
    12.93  CSS, theme and style  /  GTK and libadwaita
    11.13  exec and dynamic linking (before main)  /  dynamic linker
     5.80  CSS, theme and style  /  GLib and GObject
     2.97  fontconfig  /  fontconfig, FreeType, HarfBuzz
     2.60  CSS, theme and style  /  C library (malloc, strings, I/O)
     2.10  icon theme and images  /  GLib and GObject
     1.80  fontconfig  /  C library (malloc, strings, I/O)
     1.70  icon theme and images  /  C library (malloc, strings, I/O)
     1.17  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     1.00  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.93  other  /  GLib and GObject
     0.77  layout and drawing  /  GTK and libadwaita
     0.70  Wayland and keymaps  /  Wayland and xkbcommon
     0.53  GObject type and class setup  /  GLib and GObject
     0.50  other  /  C library (malloc, strings, I/O)

Threads (ms per run): .hello-adw-wrap 46.0, [pango] fontcon 4.4, gdbus 0.1, pool-0 0.1, hello-adw 0.1, pool-spawner 0.0

Leaf libraries (self ms per run):
    13.83  libgtk-4.so.1.2200.4
    11.70  ld-linux-x86-64.so.2
     8.73  libglib-2.0.so.0.8800.3
     8.03  libc.so.6
     2.97  libfontconfig.so.1.16.1
     1.07  libexpat.so.1.13.0
     0.97  libgobject-2.0.so.0.8800.3
     0.83  libfreetype.so.6.20.6
     0.70  libxkbcommon.so.0.13.2
     0.70  libgio-2.0.so.0.8800.3
     0.40  libcairo.so.2.11804.4
     0.27  libharfbuzz.so.0.61451.0
     0.17  libpixman-1.so.0.46.4
     0.10  libadwaita-1.so.0
     0.07  libtasn1.so.6.6.5

Leaf functions (self ms per run):
     6.70  do_lookup_x  (ld-linux-x86-64.so.2)
     2.10  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     2.10  subdivide_infos  (libgtk-4.so.1.2200.4)
     1.47  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     1.40  gtk_css_tokenizer_read_name  (libgtk-4.so.1.2200.4)
     1.27  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     1.20  _int_malloc  (libc.so.6)
     1.17  strcmp  (ld-linux-x86-64.so.2)
     1.17  g_hash_table_resize  (libglib-2.0.so.0.8800.3)
     1.13  gtk_css_parser_ensure_token  (libgtk-4.so.1.2200.4)
     1.10  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     1.00  _gtk_css_selector_compare  (libgtk-4.so.1.2200.4)
     0.93  __strcmp_evex  (libc.so.6)
     0.83  gtk_css_tokenizer_read_token  (libgtk-4.so.1.2200.4)
     0.77  __libc_malloc2  (libc.so.6)
     0.67  _int_free_chunk  (libc.so.6)
     0.63  g_str_hash  (libglib-2.0.so.0.8800.3)
     0.63  gtk_css_selector_class_add_specificity  (libgtk-4.so.1.2200.4)
     0.57  gtk_css_selector_hash_one  (libgtk-4.so.1.2200.4)
     0.57  gvdb_table_lookup  (libgio-2.0.so.0.8800.3)
     0.53  malloc  (libc.so.6)
     0.53  realloc  (libc.so.6)
     0.53  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.47  check_match  (ld-linux-x86-64.so.2)
     0.47  cfree@GLIBC_2.2.5  (libc.so.6)
     0.40  gtk_css_parser_peek_token  (libgtk-4.so.1.2200.4)
     0.40  gtk_css_selector_equal  (libgtk-4.so.1.2200.4)
     0.37  comp_class  (libgtk-4.so.1.2200.4)
     0.37  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.33  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)


<a id="qtwidgets"></a>

## qtwidgets: 3 runs, first frame (under perf) median 52.9 ms
CPU to first frame (schedstat, all threads) median 56.5 ms: user (sampled) 37.9 ms, kernel and unsampled 18.8 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| CSS, theme and style | 8.50 | 22.5% |
| fontconfig | 7.67 | 20.3% |
| Qt plugins (dlopen, metadata) | 6.33 | 16.8% |
| Wayland and keymaps | 3.60 | 9.5% |
| exec and dynamic linking (before main) | 3.37 | 8.9% |
| fonts: shaping and glyphs | 2.90 | 7.7% |
| GObject type and class setup | 1.67 | 4.4% |
| D-Bus, portals and accessibility | 1.43 | 3.8% |
| app and toolkit start | 1.07 | 2.8% |
| other | 0.60 | 1.6% |
| GIO and GTK modules | 0.30 | 0.8% |
| layout and drawing | 0.17 | 0.4% |
| library constructors (before main) | 0.10 | 0.3% |
| icon theme and images | 0.07 | 0.2% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 10.00 | 26.5% |
| C library (malloc, strings, I/O) | 9.60 | 25.4% |
| GLib and GObject | 5.37 | 14.2% |
| fontconfig, FreeType, HarfBuzz | 5.10 | 13.5% |
| GTK and libadwaita | 3.07 | 8.1% |
| Qt | 2.67 | 7.1% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.10 | 2.9% |
| Wayland and xkbcommon | 0.63 | 1.7% |
| D-Bus and a11y libraries | 0.20 | 0.5% |
| other | 0.03 | 0.1% |

Largest purpose x mechanism cells (ms per run):
     5.90  Qt plugins (dlopen, metadata)  /  dynamic linker
     4.13  fontconfig  /  fontconfig, FreeType, HarfBuzz
     3.37  exec and dynamic linking (before main)  /  dynamic linker
     2.87  CSS, theme and style  /  C library (malloc, strings, I/O)
     2.80  CSS, theme and style  /  GTK and libadwaita
     2.63  CSS, theme and style  /  GLib and GObject
     2.47  fontconfig  /  C library (malloc, strings, I/O)
     2.33  Wayland and keymaps  /  C library (malloc, strings, I/O)
     1.37  fonts: shaping and glyphs  /  Qt
     1.17  GObject type and class setup  /  GLib and GObject
     1.07  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.97  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     0.63  Wayland and keymaps  /  Wayland and xkbcommon
     0.50  D-Bus, portals and accessibility  /  C library (malloc, strings, I/O)
     0.43  app and toolkit start  /  GLib and GObject

Threads (ms per run): .hello-qtwidget 33.7, [pango] fontcon 3.4, gdbus 0.3, QDBusConnection 0.3, hello-qtwidgets 0.1

Leaf libraries (self ms per run):
    10.00  ld-linux-x86-64.so.2
     9.60  libc.so.6
     4.20  libglib-2.0.so.0.8800.3
     4.13  libfontconfig.so.1.16.1
     3.00  libgtk-3.so.0.2420.32
     1.60  libQt6Core.so.6.12.0
     1.10  libexpat.so.1.13.0
     0.93  libgobject-2.0.so.0.8800.3
     0.83  libQt6Gui.so.6.12.0
     0.50  libfreetype.so.6.20.6
     0.40  libxkbcommon.so.0.13.2
     0.30  libharfbuzz.so.0.61451.0
     0.23  libgio-2.0.so.0.8800.3
     0.17  libdbus-1.so.3.38.3
     0.13  libwayland-cursor.so.0.26.0

Leaf functions (self ms per run):
     5.00  do_lookup_x  (ld-linux-x86-64.so.2)
     1.63  strcmp  (ld-linux-x86-64.so.2)
     1.33  _IO_fread  (libc.so.6)
     1.17  __libc_malloc2  (libc.so.6)
     1.10  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     1.00  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     1.00  _int_free_chunk  (libc.so.6)
     0.93  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     0.93  _int_malloc  (libc.so.6)
     0.73  check_match  (ld-linux-x86-64.so.2)
     0.67  subdivide_infos  (libgtk-3.so.0.2420.32)
     0.60  __strcmp_evex  (libc.so.6)
     0.57  __popcountdi2  (libfontconfig.so.1.16.1)
     0.50  malloc  (libc.so.6)
     0.50  __strlen_evex512  (libc.so.6)
     0.43  g_str_hash  (libglib-2.0.so.0.8800.3)
     0.40  g_mutex_unlock  (libglib-2.0.so.0.8800.3)
     0.37  ucstricmp(long long, char16_t const*, long long, char16_t const*)  (libQt6Core.so.6.12.0)
     0.33  __libc_calloc2  (libc.so.6)
     0.30  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.30  normal_updatePosition  (libexpat.so.1.13.0)
     0.30  FcPatternObjectPosition  (libfontconfig.so.1.16.1)
     0.30  FcFontSetList  (libfontconfig.so.1.16.1)
     0.27  g_hash_table_resize  (libglib-2.0.so.0.8800.3)
     0.27  getenv  (libc.so.6)
     0.27  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.27  cfree@GLIBC_2.2.5  (libc.so.6)
     0.27  FcLangCompare  (libfontconfig.so.1.16.1)
     0.23  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.23  gtk_css_selector_name_add_specificity  (libgtk-3.so.0.2420.32)


<a id="qtwidgets.lean"></a>

## qtwidgets.lean: 3 runs, first frame (under perf) median 29.2 ms
CPU to first frame (schedstat, all threads) median 28.7 ms: user (sampled) 15.0 ms, kernel and unsampled 14.3 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| fontconfig | 7.07 | 47.0% |
| exec and dynamic linking (before main) | 3.23 | 21.5% |
| fonts: shaping and glyphs | 2.17 | 14.4% |
| Qt plugins (dlopen, metadata) | 0.80 | 5.3% |
| Wayland and keymaps | 0.73 | 4.9% |
| D-Bus, portals and accessibility | 0.40 | 2.7% |
| other | 0.20 | 1.3% |
| layout and drawing | 0.20 | 1.3% |
| app and toolkit start | 0.17 | 1.1% |
| library constructors (before main) | 0.07 | 0.4% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| fontconfig, FreeType, HarfBuzz | 4.53 | 30.2% |
| dynamic linker | 4.13 | 27.5% |
| C library (malloc, strings, I/O) | 2.87 | 19.1% |
| Qt | 2.20 | 14.6% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.00 | 6.7% |
| D-Bus and a11y libraries | 0.23 | 1.6% |
| stamps.so (measurement) | 0.03 | 0.2% |
| GLib and GObject | 0.03 | 0.2% |

Largest purpose x mechanism cells (ms per run):
     4.20  fontconfig  /  fontconfig, FreeType, HarfBuzz
     3.23  exec and dynamic linking (before main)  /  dynamic linker
     1.90  fontconfig  /  C library (malloc, strings, I/O)
     1.43  fonts: shaping and glyphs  /  Qt
     0.97  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     0.63  Qt plugins (dlopen, metadata)  /  dynamic linker
     0.40  fonts: shaping and glyphs  /  C library (malloc, strings, I/O)
     0.33  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     0.27  Wayland and keymaps  /  C library (malloc, strings, I/O)
     0.23  D-Bus, portals and accessibility  /  D-Bus and a11y libraries
     0.23  Wayland and keymaps  /  Qt
     0.20  Wayland and keymaps  /  dynamic linker
     0.20  layout and drawing  /  Qt
     0.13  D-Bus, portals and accessibility  /  Qt
     0.13  other  /  C library (malloc, strings, I/O)

Threads (ms per run): .hello-qtwidget 14.7, QDBusConnection 0.3, hello-qtwidgets 0.1

Leaf libraries (self ms per run):
     4.20  libfontconfig.so.1.16.1
     4.13  ld-linux-x86-64.so.2
     2.87  libc.so.6
     1.43  libQt6Core.so.6.12.0
     1.00  libexpat.so.1.13.0
     0.57  libQt6Gui.so.6.12.0
     0.23  libdbus-1.so.3.38.3
     0.20  libfreetype.so.6.20.6
     0.13  libharfbuzz.so.0.61451.0
     0.10  libQt6WaylandClient.so.6.12.0
     0.07  libQt6DBus.so.6.12.0
     0.03  stamps.so
     0.03  libQt6Widgets.so.6.12.0
     0.03  libglib-2.0.so.0.8800.3

Leaf functions (self ms per run):
     1.80  do_lookup_x  (ld-linux-x86-64.so.2)
     0.63  strcmp  (ld-linux-x86-64.so.2)
     0.53  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     0.43  check_match  (ld-linux-x86-64.so.2)
     0.43  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     0.43  __popcountdi2  (libfontconfig.so.1.16.1)
     0.43  QUnicodeTables::caseConversion(char32_t)  (libQt6Core.so.6.12.0)
     0.40  __libc_malloc2  (libc.so.6)
     0.37  _int_free_chunk  (libc.so.6)
     0.33  ucstricmp(long long, char16_t const*, long long, char16_t const*)  (libQt6Core.so.6.12.0)
     0.30  __strcmp_evex  (libc.so.6)
     0.30  _int_malloc  (libc.so.6)
     0.27  FcStrBufChar  (libfontconfig.so.1.16.1)
     0.27  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.27  FcPatternObjectPosition  (libfontconfig.so.1.16.1)
     0.27  FcLangSetIndex  (libfontconfig.so.1.16.1)
     0.23  __libc_calloc2  (libc.so.6)
     0.23  FcSortCompare  (libfontconfig.so.1.16.1)
     0.17  normal_contentTok  (libexpat.so.1.13.0)
     0.17  FcStrHashIgnoreCase  (libfontconfig.so.1.16.1)
     0.17  FcValueCanonicalize  (libfontconfig.so.1.16.1)
     0.17  __strlen_evex512  (libc.so.6)
     0.17  FcLangSetHasLang  (libfontconfig.so.1.16.1)
     0.17  FcCompareValueList  (libfontconfig.so.1.16.1)
     0.17  fullConvertCase(char32_t, QUnicodeTables::Case)  (libQt6Core.so.6.12.0)
     0.17  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.17  FcLangCompare  (libfontconfig.so.1.16.1)
     0.13  _dl_addr  (libc.so.6)
     0.13  normal_updatePosition  (libexpat.so.1.13.0)
     0.10  __memmove_avx512_unaligned_erms  (libc.so.6)


<a id="qtquick"></a>

## qtquick: 3 runs, first frame (under perf) median 102.4 ms
CPU to first frame (schedstat, all threads) median 104.6 ms: user (sampled) 71.8 ms, kernel and unsampled 33.5 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| GPU: driver code (other) | 9.20 | 12.8% |
| Qt plugins (dlopen, metadata) | 9.13 | 12.8% |
| fontconfig | 8.60 | 12.0% |
| CSS, theme and style | 8.47 | 11.8% |
| QML engine and Quick Controls | 8.27 | 11.5% |
| GPU: EGL and GL context | 7.87 | 11.0% |
| exec and dynamic linking (before main) | 6.23 | 8.7% |
| Wayland and keymaps | 4.50 | 6.3% |
| fonts: shaping and glyphs | 2.47 | 3.4% |
| GPU: Vulkan instance and devices | 1.60 | 2.2% |
| D-Bus, portals and accessibility | 1.33 | 1.9% |
| other | 0.93 | 1.3% |
| GObject type and class setup | 0.87 | 1.2% |
| Qt Quick items and scene graph | 0.60 | 0.8% |
| app and toolkit start | 0.47 | 0.7% |
| GPU: rendering the frame | 0.40 | 0.6% |
| GIO and GTK modules | 0.30 | 0.4% |
| GPU: pipelines and shaders | 0.27 | 0.4% |
| library constructors (before main) | 0.10 | 0.1% |
| layout and drawing | 0.03 | 0.0% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 23.63 | 33.0% |
| C library (malloc, strings, I/O) | 15.07 | 21.0% |
| Qt | 9.60 | 13.4% |
| fontconfig, FreeType, HarfBuzz | 6.20 | 8.7% |
| GPU drivers (Mesa, LLVM, NVIDIA) | 5.87 | 8.2% |
| GLib and GObject | 4.20 | 5.9% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 2.80 | 3.9% |
| GTK and libadwaita | 2.67 | 3.7% |
| Wayland and xkbcommon | 0.73 | 1.0% |
| other | 0.57 | 0.8% |
| D-Bus and a11y libraries | 0.30 | 0.4% |

Largest purpose x mechanism cells (ms per run):
     8.57  Qt plugins (dlopen, metadata)  /  dynamic linker
     7.23  GPU: EGL and GL context  /  dynamic linker
     6.83  QML engine and Quick Controls  /  Qt
     6.23  exec and dynamic linking (before main)  /  dynamic linker
     5.20  GPU: driver code (other)  /  GPU drivers (Mesa, LLVM, NVIDIA)
     5.03  fontconfig  /  fontconfig, FreeType, HarfBuzz
     3.27  CSS, theme and style  /  C library (malloc, strings, I/O)
     2.80  GPU: driver code (other)  /  C library (malloc, strings, I/O)
     2.63  fontconfig  /  C library (malloc, strings, I/O)
     2.60  CSS, theme and style  /  GLib and GObject
     2.50  Wayland and keymaps  /  C library (malloc, strings, I/O)
     2.50  CSS, theme and style  /  GTK and libadwaita
     1.17  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     1.10  GPU: driver code (other)  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     1.07  Wayland and keymaps  /  dynamic linker

Threads (ms per run): .hello-qtquick- 53.3, QQmlThread 8.9, QSGRenderThread 4.7, [pango] fontcon 3.7, QDBusConnection 0.3, gdbus 0.3, hello-qtqu:sh0 0.2, hello-qtqu:gl0 0.1, hello-qtquick 0.0

Leaf libraries (self ms per run):
    23.63  ld-linux-x86-64.so.2
    14.97  libc.so.6
     5.03  libfontconfig.so.1.16.1
     4.07  libQt6Qml.so.6.12.0
     4.03  libQt6Core.so.6.12.0
     3.60  libglib-2.0.so.0.8800.3
     2.80  libexpat.so.1.13.0
     2.63  libgallium-26.2.6.so
     2.57  libgtk-3.so.0.2420.32
     2.57  libLLVM.so.21.1
     0.87  libQt6Gui.so.6.12.0
     0.67  libfreetype.so.6.20.6
     0.57  libxkbcommon.so.0.13.2
     0.57  libEGL_mesa.so.0.0.0
     0.50  libgobject-2.0.so.0.8800.3

Leaf functions (self ms per run):
    12.27  do_lookup_x  (ld-linux-x86-64.so.2)
     3.27  strcmp  (ld-linux-x86-64.so.2)
     2.93  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     2.60  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     2.30  __libc_malloc2  (libc.so.6)
     1.57  QQmlMetaTypeData::registerType(QQmlTypePrivate*)  (libQt6Qml.so.6.12.0)
     1.47  __strcmp_evex  (libc.so.6)
     1.43  _IO_fread  (libc.so.6)
     1.43  _int_free_chunk  (libc.so.6)
     1.33  unsigned int llvm::ComputeMappedEditDistance<char, llvm::ComputeEditDistance<char>(llvm::ArrayRef<char>, llvm::ArrayRef<char>, bool, unsigned int)::{lambda(char const&)#1}>(llvm::ArrayRef<char>, llvm::ArrayRef<char>, llvm::ComputeEditDistance<char>(llvm::ArrayRef<char>, llvm::ArrayRef<char>, bool, unsigned int)::{lambda(char const&)#1}, bool, unsigned int) [clone .isra.0]  (libLLVM.so.21.1)
     1.13  _int_malloc  (libc.so.6)
     1.03  check_match  (ld-linux-x86-64.so.2)
     0.93  __strlen_evex512  (libc.so.6)
     0.67  malloc  (libc.so.6)
     0.63  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     0.53  __popcountdi2  (libfontconfig.so.1.16.1)
     0.47  _gtk_css_selector_compare  (libgtk-3.so.0.2420.32)
     0.47  cfree@GLIBC_2.2.5  (libc.so.6)
     0.47  _dl_addr  (libc.so.6)
     0.43  QtPrivate::QTypeNormalizer::normalizeType(char const*, char const*, bool)  (libQt6Core.so.6.12.0)
     0.43  FcLangSetHasLang  (libfontconfig.so.1.16.1)
     0.40  _dl_lookup_map  (ld-linux-x86-64.so.2)
     0.40  normal_contentTok  (libexpat.so.1.13.0)
     0.40  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.40  __libc_calloc2  (libc.so.6)
     0.37  g_str_hash  (libglib-2.0.so.0.8800.3)
     0.37  normal_updatePosition  (libexpat.so.1.13.0)
     0.37  normal_getAtts  (libexpat.so.1.13.0)
     0.37  subdivide_infos  (libgtk-3.so.0.2420.32)
     0.33  FcStrBufChar  (libfontconfig.so.1.16.1)


<a id="qtquick.lean"></a>

## qtquick.lean: 3 runs, first frame (under perf) median 53.0 ms
CPU to first frame (schedstat, all threads) median 47.2 ms: user (sampled) 28.6 ms, kernel and unsampled 18.3 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| QML engine and Quick Controls | 8.07 | 27.7% |
| fontconfig | 6.83 | 23.4% |
| exec and dynamic linking (before main) | 6.13 | 21.0% |
| Qt plugins (dlopen, metadata) | 2.93 | 10.1% |
| fonts: shaping and glyphs | 2.43 | 8.3% |
| Wayland and keymaps | 1.47 | 5.0% |
| D-Bus, portals and accessibility | 0.37 | 1.3% |
| Qt Quick items and scene graph | 0.30 | 1.0% |
| app and toolkit start | 0.20 | 0.7% |
| other | 0.20 | 0.7% |
| library constructors (before main) | 0.17 | 0.6% |
| layout and drawing | 0.07 | 0.2% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 9.90 | 33.9% |
| Qt | 9.07 | 31.1% |
| C library (malloc, strings, I/O) | 4.30 | 14.7% |
| fontconfig, FreeType, HarfBuzz | 3.80 | 13.0% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.30 | 4.5% |
| other | 0.43 | 1.5% |
| D-Bus and a11y libraries | 0.13 | 0.5% |
| Wayland and xkbcommon | 0.10 | 0.3% |
| GLib and GObject | 0.07 | 0.2% |
| the program itself | 0.03 | 0.1% |
| stamps.so (measurement) | 0.03 | 0.1% |

Largest purpose x mechanism cells (ms per run):
     6.50  QML engine and Quick Controls  /  Qt
     6.13  exec and dynamic linking (before main)  /  dynamic linker
     3.40  fontconfig  /  fontconfig, FreeType, HarfBuzz
     2.63  Qt plugins (dlopen, metadata)  /  dynamic linker
     2.13  fontconfig  /  C library (malloc, strings, I/O)
     1.60  fonts: shaping and glyphs  /  Qt
     1.30  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     1.17  QML engine and Quick Controls  /  C library (malloc, strings, I/O)
     1.03  Wayland and keymaps  /  dynamic linker
     0.40  QML engine and Quick Controls  /  other
     0.40  fonts: shaping and glyphs  /  C library (malloc, strings, I/O)
     0.40  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     0.23  Qt Quick items and scene graph  /  Qt
     0.20  Wayland and keymaps  /  C library (malloc, strings, I/O)
     0.17  Qt plugins (dlopen, metadata)  /  C library (malloc, strings, I/O)

Threads (ms per run): .hello-qtquick- 19.7, QQmlThread 9.2, QDBusConnection 0.3, hello-qtquick 0.0

Leaf libraries (self ms per run):
     9.90  ld-linux-x86-64.so.2
     4.13  libc.so.6
     4.07  libQt6Core.so.6.12.0
     4.00  libQt6Qml.so.6.12.0
     3.40  libfontconfig.so.1.16.1
     1.30  libexpat.so.1.13.0
     0.60  libQt6Gui.so.6.12.0
     0.40  libcrypto.so.3
     0.23  libfreetype.so.6.20.6
     0.17  libstdc++.so.6.0.36
     0.17  libharfbuzz.so.0.61451.0
     0.13  libdbus-1.so.3.38.3
     0.07  libQt6DBus.so.6.12.0
     0.07  libwayland-client.so.0.26.0
     0.07  libQt6QuickControls2Fusion.so.6.12.0

Leaf functions (self ms per run):
     3.57  do_lookup_x  (ld-linux-x86-64.so.2)
     2.37  strcmp  (ld-linux-x86-64.so.2)
     1.63  QQmlMetaTypeData::registerType(QQmlTypePrivate*)  (libQt6Qml.so.6.12.0)
     1.23  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     0.93  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     0.83  check_match  (ld-linux-x86-64.so.2)
     0.70  __libc_malloc2  (libc.so.6)
     0.50  _int_malloc  (libc.so.6)
     0.50  __popcountdi2  (libfontconfig.so.1.16.1)
     0.43  ucstricmp(long long, char16_t const*, long long, char16_t const*)  (libQt6Core.so.6.12.0)
     0.40  malloc  (libc.so.6)
     0.40  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.30  FcStrHashIgnoreCase  (libfontconfig.so.1.16.1)
     0.27  __strlen_evex512  (libc.so.6)
     0.27  __strcmp_evex  (libc.so.6)
     0.27  _int_free_chunk  (libc.so.6)
     0.23  __libc_calloc2  (libc.so.6)
     0.23  FcLangSetIndex  (libfontconfig.so.1.16.1)
     0.23  QUnicodeTables::caseConversion(char32_t)  (libQt6Core.so.6.12.0)
     0.20  qt_hash(QStringView, unsigned int)  (libQt6Core.so.6.12.0)
     0.20  normal_contentTok  (libexpat.so.1.13.0)
     0.20  FcSortCompare  (libfontconfig.so.1.16.1)
     0.20  QtPrivate::QTypeNormalizer::normalizeType(char const*, char const*, bool)  (libQt6Core.so.6.12.0)
     0.17  _dl_lookup_map  (ld-linux-x86-64.so.2)
     0.17  _dl_name_match_p  (ld-linux-x86-64.so.2)
     0.17  QQmlMetaTypeData::findTypeModule(QString const&, QTypeRevision)  (libQt6Qml.so.6.12.0)
     0.17  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.17  FcLangCompare  (libfontconfig.so.1.16.1)
     0.17  fullConvertCase(char32_t, QUnicodeTables::Case)  (libQt6Core.so.6.12.0)
     0.17  _dl_map_object_deps  (ld-linux-x86-64.so.2)


<a id="gnome-text-editor"></a>

## gnome-text-editor: 3 runs, first frame (under perf) median 173.2 ms
CPU to first frame (schedstat, all threads) median 178.1 ms: user (sampled) 124.1 ms, kernel and unsampled 54.0 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| GPU: Vulkan instance and devices | 30.63 | 24.7% |
| CSS, theme and style | 28.13 | 22.7% |
| exec and dynamic linking (before main) | 11.87 | 9.6% |
| GPU: driver code (other) | 8.03 | 6.5% |
| fontconfig | 6.87 | 5.5% |
| fonts: shaping and glyphs | 5.77 | 4.7% |
| icon theme and images | 5.17 | 4.2% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 4.57 | 3.7% |
| GObject type and class setup | 3.63 | 2.9% |
| app and toolkit start | 3.30 | 2.7% |
| editor component (syntax, buffer, spell checking) | 3.17 | 2.6% |
| other | 3.03 | 2.4% |
| D-Bus, portals and accessibility | 2.47 | 2.0% |
| layout and drawing | 2.10 | 1.7% |
| GIO and GTK modules | 2.07 | 1.7% |
| Wayland and keymaps | 1.23 | 1.0% |
| GPU: EGL and GL context | 1.13 | 0.9% |
| library constructors (before main) | 0.33 | 0.3% |
| settings (GSettings, dconf, KConfig) | 0.23 | 0.2% |
| GPU: rendering the frame | 0.10 | 0.1% |
| GPU: pipelines and shaders | 0.07 | 0.1% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 31.30 | 25.3% |
| GTK and libadwaita | 22.70 | 18.3% |
| GLib and GObject | 21.70 | 17.5% |
| C library (malloc, strings, I/O) | 21.17 | 17.1% |
| GPU drivers (Mesa, LLVM, NVIDIA) | 11.83 | 9.6% |
| fontconfig, FreeType, HarfBuzz | 8.87 | 7.2% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 4.60 | 3.7% |
| other | 0.97 | 0.8% |
| Wayland and xkbcommon | 0.70 | 0.6% |
| stamps.so (measurement) | 0.07 | 0.1% |

Largest purpose x mechanism cells (ms per run):
    18.33  CSS, theme and style  /  GTK and libadwaita
    16.03  GPU: Vulkan instance and devices  /  dynamic linker
    11.83  exec and dynamic linking (before main)  /  dynamic linker
     9.57  GPU: Vulkan instance and devices  /  GPU drivers (Mesa, LLVM, NVIDIA)
     6.63  CSS, theme and style  /  GLib and GObject
     4.97  GPU: Vulkan instance and devices  /  C library (malloc, strings, I/O)
     4.47  fontconfig  /  fontconfig, FreeType, HarfBuzz
     4.40  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     3.33  GPU: driver code (other)  /  XML parsing (expat: fontconfig's and Mesa's configuration)
     3.17  CSS, theme and style  /  C library (malloc, strings, I/O)
     2.87  icon theme and images  /  GLib and GObject
     2.80  GObject type and class setup  /  GLib and GObject
     2.73  UI built from XML (GtkBuilder templates, KXMLGUI)  /  GLib and GObject
     2.50  GPU: driver code (other)  /  C library (malloc, strings, I/O)
     2.17  GPU: driver code (other)  /  GPU drivers (Mesa, LLVM, NVIDIA)

Threads (ms per run): .gnome-text-edi 116.7, [pango] fontcon 4.9, gdbus 1.0, dconf worker 0.7, pool-0 0.3, [gtksourceview- 0.1, pool-1 0.1, pool-2 0.1, gnome-text-edit 0.0

Leaf libraries (self ms per run):
    31.30  ld-linux-x86-64.so.2
    21.70  libgtk-4.so.1.2200.4
    20.93  libc.so.6
    13.73  libglib-2.0.so.0.8800.3
     6.13  libgobject-2.0.so.0.8800.3
     5.53  libnvidia-glcore.so.615.78.08
     4.60  libexpat.so.1.13.0
     4.47  libfontconfig.so.1.16.1
     4.03  libnvidia-gpucomp.so.615.78.08
     2.23  libharfbuzz.so.0.61451.0
     1.83  libgio-2.0.so.0.8800.3
     1.43  libfreetype.so.6.20.6
     0.67  libvulkan_radeon.so
     0.63  libxml2.so.16.1.4
     0.63  libpango-1.0.so.0.5701.0

Leaf functions (self ms per run):
    19.47  do_lookup_x  (ld-linux-x86-64.so.2)
     5.50  [unknown]  (libnvidia-glcore.so.615.78.08)
     3.93  [unknown]  (libnvidia-gpucomp.so.615.78.08)
     3.73  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     2.93  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     2.87  strcmp  (ld-linux-x86-64.so.2)
     2.83  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     2.30  __strcmp_evex  (libc.so.6)
     2.13  _int_malloc  (libc.so.6)
     2.03  subdivide_infos  (libgtk-4.so.1.2200.4)
     1.60  gtk_css_parser_ensure_token  (libgtk-4.so.1.2200.4)
     1.60  gtk_css_selector_tree_get_change  (libgtk-4.so.1.2200.4)
     1.33  gtk_css_tokenizer_read_name  (libgtk-4.so.1.2200.4)
     1.27  __libc_malloc2  (libc.so.6)
     1.27  _int_free_chunk  (libc.so.6)
     1.27  __GI_____strtoull_l_internal  (libc.so.6)
     1.23  gvdb_table_lookup  (libgio-2.0.so.0.8800.3)
     1.20  __strlen_evex512  (libc.so.6)
     1.10  g_hash_table_resize  (libglib-2.0.so.0.8800.3)
     1.07  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     1.00  check_match  (ld-linux-x86-64.so.2)
     0.93  malloc  (libc.so.6)
     0.93  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.90  _gtk_css_selector_compare  (libgtk-4.so.1.2200.4)
     0.83  type_node_check_conformities_UorL  (libgobject-2.0.so.0.8800.3)
     0.80  __libc_calloc2  (libc.so.6)
     0.80  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.80  gtk_css_selector_equal  (libgtk-4.so.1.2200.4)
     0.77  normal_contentTok  (libexpat.so.1.13.0)
     0.77  realloc  (libc.so.6)


<a id="gnome-text-editor.lean"></a>

## gnome-text-editor.lean: 3 runs, first frame (under perf) median 98.3 ms
CPU to first frame (schedstat, all threads) median 104.1 ms: user (sampled) 81.3 ms, kernel and unsampled 22.2 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| CSS, theme and style | 27.97 | 34.3% |
| exec and dynamic linking (before main) | 11.80 | 14.5% |
| fontconfig | 8.53 | 10.5% |
| fonts: shaping and glyphs | 5.30 | 6.5% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 4.83 | 5.9% |
| icon theme and images | 4.63 | 5.7% |
| app and toolkit start | 3.30 | 4.0% |
| editor component (syntax, buffer, spell checking) | 3.23 | 4.0% |
| GObject type and class setup | 2.60 | 3.2% |
| other | 2.37 | 2.9% |
| layout and drawing | 2.30 | 2.8% |
| GIO and GTK modules | 1.97 | 2.4% |
| Wayland and keymaps | 1.30 | 1.6% |
| D-Bus, portals and accessibility | 0.77 | 0.9% |
| library constructors (before main) | 0.37 | 0.4% |
| settings (GSettings, dconf, KConfig) | 0.23 | 0.3% |
| GPU: rendering the frame | 0.03 | 0.0% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| GTK and libadwaita | 21.17 | 26.0% |
| GLib and GObject | 21.00 | 25.8% |
| dynamic linker | 13.77 | 16.9% |
| C library (malloc, strings, I/O) | 13.33 | 16.4% |
| fontconfig, FreeType, HarfBuzz | 9.57 | 11.7% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 0.97 | 1.2% |
| other | 0.87 | 1.1% |
| Wayland and xkbcommon | 0.83 | 1.0% |
| stamps.so (measurement) | 0.03 | 0.0% |

Largest purpose x mechanism cells (ms per run):
    17.17  CSS, theme and style  /  GTK and libadwaita
    11.80  exec and dynamic linking (before main)  /  dynamic linker
     7.20  CSS, theme and style  /  GLib and GObject
     5.27  fontconfig  /  fontconfig, FreeType, HarfBuzz
     4.30  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     3.60  CSS, theme and style  /  C library (malloc, strings, I/O)
     3.17  UI built from XML (GtkBuilder templates, KXMLGUI)  /  GLib and GObject
     2.47  icon theme and images  /  GLib and GObject
     2.30  fontconfig  /  C library (malloc, strings, I/O)
     2.13  GObject type and class setup  /  GLib and GObject
     1.90  editor component (syntax, buffer, spell checking)  /  C library (malloc, strings, I/O)
     1.87  app and toolkit start  /  GLib and GObject
     1.80  icon theme and images  /  C library (malloc, strings, I/O)
     1.70  GIO and GTK modules  /  dynamic linker
     1.50  other  /  GLib and GObject

Threads (ms per run): .gnome-text-edi 73.7, [pango] fontcon 6.4, gdbus 0.5, pool-0 0.3, dconf worker 0.2, [gtksourceview- 0.2, pool-2 0.1, gnome-text-edit 0.1, pool-1 0.1

Leaf libraries (self ms per run):
    20.00  libgtk-4.so.1.2200.4
    13.90  libglib-2.0.so.0.8800.3
    13.77  ld-linux-x86-64.so.2
    13.13  libc.so.6
     5.63  libgobject-2.0.so.0.8800.3
     5.27  libfontconfig.so.1.16.1
     1.80  libfreetype.so.6.20.6
     1.70  libharfbuzz.so.0.61451.0
     1.47  libgio-2.0.so.0.8800.3
     0.97  libexpat.so.1.13.0
     0.77  libpango-1.0.so.0.5701.0
     0.63  libxkbcommon.so.0.13.2
     0.57  libcairo.so.2.11804.4
     0.50  libxml2.so.16.1.4
     0.33  libpixman-1.so.0.46.4

Leaf functions (self ms per run):
     8.73  do_lookup_x  (ld-linux-x86-64.so.2)
     3.67  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     2.03  subdivide_infos  (libgtk-4.so.1.2200.4)
     1.73  __strcmp_evex  (libc.so.6)
     1.63  _int_malloc  (libc.so.6)
     1.53  strcmp  (ld-linux-x86-64.so.2)
     1.40  gtk_css_tokenizer_read_name  (libgtk-4.so.1.2200.4)
     1.37  gtk_css_parser_ensure_token  (libgtk-4.so.1.2200.4)
     1.33  __libc_malloc2  (libc.so.6)
     1.30  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     1.17  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     1.17  type_node_check_conformities_UorL  (libgobject-2.0.so.0.8800.3)
     1.17  gtk_css_selector_tree_get_change  (libgtk-4.so.1.2200.4)
     1.00  FcCompareValueList  (libfontconfig.so.1.16.1)
     0.97  _gtk_css_selector_compare  (libgtk-4.so.1.2200.4)
     0.97  g_hash_table_resize  (libglib-2.0.so.0.8800.3)
     0.93  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     0.87  _int_free_chunk  (libc.so.6)
     0.83  gvdb_table_lookup  (libgio-2.0.so.0.8800.3)
     0.80  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.73  g_str_hash  (libglib-2.0.so.0.8800.3)
     0.70  check_match  (ld-linux-x86-64.so.2)
     0.70  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.70  cfree@GLIBC_2.2.5  (libc.so.6)
     0.67  __strlen_evex512  (libc.so.6)
     0.67  gtk_css_tokenizer_read_token  (libgtk-4.so.1.2200.4)
     0.67  gtk_css_selector_tree_match  (libgtk-4.so.1.2200.4)
     0.67  FcCompare  (libfontconfig.so.1.16.1)
     0.63  malloc  (libc.so.6)
     0.60  realloc  (libc.so.6)


<a id="kwrite"></a>

## kwrite: 3 runs, first frame (under perf) median 130.2 ms
CPU to first frame (schedstat, all threads) median 120.2 ms: user (sampled) 87.5 ms, kernel and unsampled 31.3 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| exec and dynamic linking (before main) | 19.57 | 22.4% |
| Qt plugins (dlopen, metadata) | 15.57 | 17.8% |
| fontconfig | 9.03 | 10.3% |
| CSS, theme and style | 9.00 | 10.3% |
| editor component (syntax, buffer, spell checking) | 7.33 | 8.4% |
| fonts: shaping and glyphs | 5.90 | 6.7% |
| Wayland and keymaps | 3.37 | 3.8% |
| settings (GSettings, dconf, KConfig) | 3.13 | 3.6% |
| app and toolkit start | 3.03 | 3.5% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 2.57 | 2.9% |
| layout and drawing | 2.27 | 2.6% |
| icon theme and images | 2.00 | 2.3% |
| D-Bus, portals and accessibility | 1.60 | 1.8% |
| GObject type and class setup | 1.30 | 1.5% |
| other | 0.87 | 1.0% |
| GIO and GTK modules | 0.57 | 0.6% |
| library constructors (before main) | 0.37 | 0.4% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 32.13 | 36.7% |
| Qt | 17.50 | 20.0% |
| C library (malloc, strings, I/O) | 14.20 | 16.2% |
| fontconfig, FreeType, HarfBuzz | 7.60 | 8.7% |
| GLib and GObject | 4.70 | 5.4% |
| other | 4.40 | 5.0% |
| GTK and libadwaita | 3.70 | 4.2% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.17 | 1.3% |
| Wayland and xkbcommon | 0.90 | 1.0% |
| GPU drivers (Mesa, LLVM, NVIDIA) | 0.67 | 0.8% |
| D-Bus and a11y libraries | 0.43 | 0.5% |
| stamps.so (measurement) | 0.07 | 0.1% |

Largest purpose x mechanism cells (ms per run):
    19.53  exec and dynamic linking (before main)  /  dynamic linker
    11.97  Qt plugins (dlopen, metadata)  /  dynamic linker
     4.73  fontconfig  /  fontconfig, FreeType, HarfBuzz
     3.13  fontconfig  /  C library (malloc, strings, I/O)
     3.13  editor component (syntax, buffer, spell checking)  /  Qt
     3.07  CSS, theme and style  /  GTK and libadwaita
     2.90  CSS, theme and style  /  C library (malloc, strings, I/O)
     2.87  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     2.83  CSS, theme and style  /  GLib and GObject
     2.57  editor component (syntax, buffer, spell checking)  /  other
     2.53  Qt plugins (dlopen, metadata)  /  Qt
     2.30  fonts: shaping and glyphs  /  Qt
     2.00  Wayland and keymaps  /  C library (malloc, strings, I/O)
     1.93  app and toolkit start  /  Qt
     1.90  layout and drawing  /  Qt

Threads (ms per run): .kwrite-wrapped 82.5, [pango] fontcon 4.1, QDBusConnection 0.5, gdbus 0.2, kwrite 0.1

Leaf libraries (self ms per run):
    32.13  ld-linux-x86-64.so.2
    13.83  libc.so.6
    12.90  libQt6Core.so.6.12.0
     4.73  libfontconfig.so.1.16.1
     3.70  libglib-2.0.so.0.8800.3
     3.53  libgtk-3.so.0.2420.32
     2.17  libQt6Gui.so.6.12.0
     1.87  libfreetype.so.6.20.6
     1.40  libQt6Widgets.so.6.12.0
     1.17  libexpat.so.1.13.0
     0.87  libgobject-2.0.so.0.8800.3
     0.80  libKF6ConfigCore.so.6.31.0
     0.77  libharfbuzz.so.0.61451.0
     0.70  libKF6SyntaxHighlighting.so.6.31.0
     0.67  libxkbcommon.so.0.13.2

Leaf functions (self ms per run):
    19.60  do_lookup_x  (ld-linux-x86-64.so.2)
     3.83  strcmp  (ld-linux-x86-64.so.2)
     3.13  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     2.57  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     2.10  __libc_malloc2  (libc.so.6)
     1.87  check_match  (ld-linux-x86-64.so.2)
     1.77  _int_malloc  (libc.so.6)
     1.20  _IO_fread  (libc.so.6)
     1.03  _int_free_chunk  (libc.so.6)
     1.00  malloc  (libc.so.6)
     0.93  QtPrivate::compareStrings(QStringView, QStringView, Qt::CaseSensitivity)  (libQt6Core.so.6.12.0)
     0.80  __strcmp_evex  (libc.so.6)
     0.70  g_hash_table_lookup  (libglib-2.0.so.0.8800.3)
     0.67  subdivide_infos  (libgtk-3.so.0.2420.32)
     0.53  ucstricmp(long long, char16_t const*, long long, char16_t const*)  (libQt6Core.so.6.12.0)
     0.50  __popcountdi2  (libfontconfig.so.1.16.1)
     0.47  _gtk_css_selector_compare  (libgtk-3.so.0.2420.32)
     0.47  __libc_calloc2  (libc.so.6)
     0.47  QXmlStreamReaderPrivate::parse()  (libQt6Core.so.6.12.0)
     0.43  QUnicodeTables::caseConversion(char32_t)  (libQt6Core.so.6.12.0)
     0.40  __strlen_evex512  (libc.so.6)
     0.40  g_hash_table_replace  (libglib-2.0.so.0.8800.3)
     0.37  __memmove_avx512_unaligned_erms  (libc.so.6)
     0.37  _int_free_create_chunk  (libc.so.6)
     0.37  ZSTD_decompressSequences_bmi2.constprop.0  (libzstd.so.1.5.7)
     0.37  QJsonPrivate::Parser::parseObject() [clone .part.0]  (libQt6Core.so.6.12.0)
     0.33  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.33  g_hash_table_insert_node  (libglib-2.0.so.0.8800.3)
     0.33  FcStrHashIgnoreCase  (libfontconfig.so.1.16.1)
     0.33  (anonymous namespace)::QResourceRoot::findNode(QString const&, QLocale const&) const  (libQt6Core.so.6.12.0)


<a id="kwrite.lean"></a>

## kwrite.lean: 3 runs, first frame (under perf) median 89.2 ms
CPU to first frame (schedstat, all threads) median 85.0 ms: user (sampled) 59.9 ms, kernel and unsampled 25.8 ms

| Purpose | user ms per run | share |
|---|---:|---:|
| exec and dynamic linking (before main) | 19.77 | 33.1% |
| fontconfig | 8.43 | 14.1% |
| editor component (syntax, buffer, spell checking) | 7.17 | 12.0% |
| Qt plugins (dlopen, metadata) | 5.47 | 9.2% |
| fonts: shaping and glyphs | 4.87 | 8.2% |
| settings (GSettings, dconf, KConfig) | 2.83 | 4.7% |
| app and toolkit start | 2.80 | 4.7% |
| UI built from XML (GtkBuilder templates, KXMLGUI) | 2.47 | 4.1% |
| icon theme and images | 1.80 | 3.0% |
| layout and drawing | 1.80 | 3.0% |
| D-Bus, portals and accessibility | 0.77 | 1.3% |
| other | 0.60 | 1.0% |
| CSS, theme and style | 0.33 | 0.6% |
| library constructors (before main) | 0.30 | 0.5% |
| Wayland and keymaps | 0.27 | 0.4% |

| Mechanism (leaf library) | user ms per run | share |
|---|---:|---:|
| dynamic linker | 22.00 | 36.9% |
| Qt | 17.93 | 30.1% |
| C library (malloc, strings, I/O) | 7.00 | 11.7% |
| fontconfig, FreeType, HarfBuzz | 6.70 | 11.2% |
| other | 3.87 | 6.5% |
| XML parsing (expat: fontconfig's and Mesa's configuration) | 1.27 | 2.1% |
| GPU drivers (Mesa, LLVM, NVIDIA) | 0.60 | 1.0% |
| D-Bus and a11y libraries | 0.27 | 0.4% |
| GLib and GObject | 0.03 | 0.1% |

Largest purpose x mechanism cells (ms per run):
    19.63  exec and dynamic linking (before main)  /  dynamic linker
     4.87  fontconfig  /  fontconfig, FreeType, HarfBuzz
     3.43  editor component (syntax, buffer, spell checking)  /  Qt
     2.53  fonts: shaping and glyphs  /  Qt
     2.33  editor component (syntax, buffer, spell checking)  /  other
     2.30  fontconfig  /  C library (malloc, strings, I/O)
     2.20  Qt plugins (dlopen, metadata)  /  dynamic linker
     2.17  app and toolkit start  /  Qt
     2.13  Qt plugins (dlopen, metadata)  /  Qt
     1.93  UI built from XML (GtkBuilder templates, KXMLGUI)  /  Qt
     1.80  fonts: shaping and glyphs  /  fontconfig, FreeType, HarfBuzz
     1.70  layout and drawing  /  Qt
     1.60  settings (GSettings, dconf, KConfig)  /  Qt
     1.40  icon theme and images  /  Qt
     1.27  fontconfig  /  XML parsing (expat: fontconfig's and Mesa's configuration)

Threads (ms per run): .kwrite-wrapped 59.1, QDBusConnection 0.4, kwrite 0.2

Leaf libraries (self ms per run):
    22.00  ld-linux-x86-64.so.2
    12.73  libQt6Core.so.6.12.0
     6.63  libc.so.6
     4.87  libfontconfig.so.1.16.1
     2.80  libQt6Gui.so.6.12.0
     1.47  libQt6Widgets.so.6.12.0
     1.27  libexpat.so.1.13.0
     1.23  libfreetype.so.6.20.6
     0.80  libKF6SyntaxHighlighting.so.6.31.0
     0.67  libKF6ConfigCore.so.6.31.0
     0.67  libQt6Xml.so.6.12.0
     0.60  libzstd.so.1.5.7
     0.57  libharfbuzz.so.0.61451.0
     0.53  libKF6TextEditor.so.6.31.0
     0.43  libpcre2-16.so.0.16.1

Leaf functions (self ms per run):
    12.87  do_lookup_x  (ld-linux-x86-64.so.2)
     2.83  strcmp  (ld-linux-x86-64.so.2)
     2.17  _dl_lookup_symbol_x  (ld-linux-x86-64.so.2)
     1.77  _dl_relocate_object_no_relro  (ld-linux-x86-64.so.2)
     1.30  check_match  (ld-linux-x86-64.so.2)
     1.17  __libc_malloc2  (libc.so.6)
     0.97  _int_malloc  (libc.so.6)
     0.90  QtPrivate::compareStrings(QStringView, QStringView, Qt::CaseSensitivity)  (libQt6Core.so.6.12.0)
     0.60  _int_free_chunk  (libc.so.6)
     0.60  __popcountdi2  (libfontconfig.so.1.16.1)
     0.57  FcSortCompare  (libfontconfig.so.1.16.1)
     0.53  __strcmp_evex  (libc.so.6)
     0.47  malloc  (libc.so.6)
     0.43  ZSTD_decompressSequences_bmi2.constprop.0  (libzstd.so.1.5.7)
     0.43  ucstricmp(long long, char16_t const*, long long, char16_t const*)  (libQt6Core.so.6.12.0)
     0.37  normal_updatePosition  (libexpat.so.1.13.0)
     0.37  accountingDiffTolerated.part.0  (libexpat.so.1.13.0)
     0.33  QCborValue::dispose()  (libQt6Core.so.6.12.0)
     0.33  normal_contentTok  (libexpat.so.1.13.0)
     0.33  QUnicodeTables::caseConversion(char32_t)  (libQt6Core.so.6.12.0)
     0.30  _dl_map_new_object  (ld-linux-x86-64.so.2)
     0.30  QUtf8::compareUtf8(QByteArrayView, QLatin1String, Qt::CaseSensitivity)  (libQt6Core.so.6.12.0)
     0.30  FcDirCacheMapFd  (libfontconfig.so.1.16.1)
     0.30  FcLangCompare  (libfontconfig.so.1.16.1)
     0.30  (anonymous namespace)::QResourceRoot::findNode(QString const&, QLocale const&) const  (libQt6Core.so.6.12.0)
     0.27  long long indexOf<QLatin1String>(QExplicitlySharedDataPointer<QCborContainerPrivate> const&, QLatin1String, bool*)  (libQt6Core.so.6.12.0)
     0.27  QJsonPrivate::Parser::parseString()  (libQt6Core.so.6.12.0)
     0.27  __strlen_evex512  (libc.so.6)
     0.27  FcFontSetList  (libfontconfig.so.1.16.1)
     0.27  FcPatternObjectPosition  (libfontconfig.so.1.16.1)

