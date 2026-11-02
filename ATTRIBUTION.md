# Attribution

Only what the licenses require when a built RustCut is passed on. `scripts/attribution.py` writes this file.

RustCut's own source code is under the MIT License, see `LICENSE`. The built programs as a whole are under the GPL-3.0-only, because
they link the libmpv and FFmpeg libraries of the system they are built on, which are usually built under the GPL (Arch and Manjaro do);
their source is at <https://github.com/vommie/rustcut>.
Long license texts stand once in `LICENSES/`.

## Named because the license asks for it

- **Slint** by SixtyFPS GmbH, used under the GPL-3.0-only (`LICENSES/GPL-3.0-only.txt`). Source: <https://github.com/slint-ui/slint>.
- **libmpv2** and **libmpv2-sys**, compiled in, under the LGPL-2.1 (`LICENSES/LGPL-2.1-only.txt`).
  Source: <https://github.com/kohsine/libmpv-rs>.
- **option-ext**, compiled in, under the MPL-2.0 (`LICENSES/MPL-2.0.txt`). Source: <https://github.com/soc/option-ext>.
- **Tabler Icons**, MIT, copyright line and text below.

## C code compiled in by crates

cargo-about only sees the crates' own licenses; these crates compile bundled C code into the programs.

- **libwebp** by Google (<https://chromium.googlesource.com/webm/libwebp>), compiled in by `libwebp-sys`, BSD-3-Clause:

```
Copyright (c) 2010, Google Inc. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of Google nor the names of its contributors may
    be used to endorse or promote products derived from this software
    without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

- **SQLite**, compiled in by `libsqlite3-sys`, is in the public domain and asks for nothing.

## BSD 2-Clause "Simplified" License

- Copyright (c) 2015 David Roundy <roundyd@physics.oregonstate.edu>

```
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the
   distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## BSD 3-Clause "New" or "Revised" License

- Copyright (c) 2011 Google Inc. All rights reserved.
- Copyright (c) 2020 Yevhenii Reizner All rights reserved.
- Copyright (c) 2026, Alexandre Bury
- Copyright © WHATWG (Apple, Google, Mozilla, Microsoft).

```
BSD 3-Clause License

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of the copyright holder nor the names of its
    contributors may be used to endorse or promote products derived
    from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS “AS IS” AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## ISC License

- Copyright (c) Hanno Braun and contributors
- Copyright © 2015, Simonas Kazlauskas

```
Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright notice
and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
```

## MIT License

- Copyright (c) 2006-2009 Graydon Hoare
- Copyright (c) 2009-2013 Mozilla Foundation
- Copyright (c) 2010 The Rust Project Developers
- Copyright (c) 2012-2013 Mozilla Foundation
- Copyright (c) 2013-2014 The Rust Project Developers.
- Copyright (c) 2013-2016 The rust-url developers
- Copyright (c) 2013-2025 The rust-url developers
- Copyright (c) 2014 Alex Crichton
- Copyright (c) 2014 Benjamin Sago
- Copyright (c) 2014 Carl Lerche and other MIO contributors
- Copyright (c) 2014 Chris Morgan and the Teepee project developers
- Copyright (c) 2014 Chris Wong
- Copyright (c) 2014 Paho Lurie-Gregg
- Copyright (c) 2014 The rusqlite developers
- Copyright (c) 2014 The Rust Project Developers
- Copyright (c) 2014-2019 Geoffroy Couprie
- Copyright (c) 2014-2020 Optimal Computing (NZ) Ltd
- Copyright (c) 2014-2022 Steven Fackler, Yuki Okushi
- Copyright (c) 2014-2026 Alex Crichton
- Copyright (c) 2014-2026 Sean McArthur
- Copyright (c) 2015
- Copyright (c) 2015 Alice Maz
- Copyright (c) 2015 Andrew Gallant
- Copyright (c) 2015 Bartłomiej Kamiński
- Copyright (c) 2015 Carl Lerche + nix-rust Authors
- Copyright (c) 2015 Dan Burkert
- Copyright (c) 2015 Danny Guo
- Copyright (c) 2015 Elinor Berger
- Copyright (c) 2015 fangyuanziti
- Copyright (c) 2015 Nicholas Allegra (comex).
- Copyright (c) 2015 nwin
- Copyright (c) 2015 Steven Allen
- Copyright (c) 2015 The cobs.rs Developers
- Copyright (c) 2015 The Gimli Developers
- Copyright (c) 2015 The procfs Developers
- Copyright (c) 2015 The quick-error Developers
- Copyright (c) 2015 The Rust Project Developers
- Copyright (c) 2015 The rust-openssl-verify Developers
- Copyright (c) 2015 Victor Berger
- Copyright (c) 2015-2017
- Copyright (c) 2015-2020 The rust-hex Developers
- Copyright (c) 2015-2022 Amod Malviya
- Copyright (c) 2016 Alex Crichton
- Copyright (c) 2016 Amanieu d'Antras
- Copyright (c) 2016 Artyom Pavlov
- Copyright (c) 2016 Jelte Fennema
- Copyright (c) 2016 Joe Wilm
- Copyright (c) 2016 Johann Tuffe
- Copyright (c) 2016 Jon Gjengset
- Copyright (c) 2016 PistonDevelopers
- Copyright (c) 2016 The fallible-streaming-iterator Developers
- Copyright (c) 2016 The Rust Project Developers
- Copyright (c) 2016 Titus Wormer <tituswormer@gmail.com>
- Copyright (c) 2016--2017
- Copyright (c) 2016--2023
- Copyright (c) 2016-2018 The gimli Developers
- Copyright (c) 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers
- Copyright (c) 2016-2021 Diggory Blake, and other contributors.
- Copyright (c) 2016-2026 The RustCrypto Project Developers
- Copyright (c) 2017 Aaron Hancock
- Copyright (c) 2017 Andrew Gallant
- Copyright (c) 2017 arc-swap developers
- Copyright (c) 2017 Artyom Pavlov
- Copyright (c) 2017 Contributors
- Copyright (c) 2017 Frommi
- Copyright (c) 2017 Gilad Naaman
- Copyright (c) 2017 Hendrik Sollich
- Copyright (c) 2017 Maiddog
- Copyright (c) 2017 Maik Klein
- Copyright (c) 2017 Matt Brubeck
- Copyright (c) 2017 Nikolai Vazquez
- Copyright (c) 2017 Robert Grosse
- Copyright (c) 2017 Ted Driggs
- Copyright (c) 2017 The Tokio Authors
- Copyright (c) 2017 tokio-jsonrpc developers
- Copyright (c) 2017-2020 The Rust Project Developers
- Copyright (c) 2017-2023 Maik Klein, Maja Kądziołka
- Copyright (c) 2017-2024 oyvindln
- Copyright (c) 2017-2025 RustCrypto Developers
- Copyright (c) 2018 Akash Kurdekar
- Copyright (c) 2018 Ashley Mannix, Christopher Armstrong, Dylan DPC, Hunar Roop Kahlon
- Copyright (c) 2018 Carl Lerche
- Copyright (c) 2018 Dan Reeves
- Copyright (c) 2018 directories-rs contributors
- Copyright (c) 2018 PistonDevelopers
- Copyright (c) 2018 Raph Levien
- Copyright (c) 2018 Reizner Evgeniy
- Copyright (c) 2018 Sam Rijs, Alex Crichton and contributors
- Copyright (c) 2018 The pin-utils authors
- Copyright (c) 2018 The Servo Project Developers
- Copyright (c) 2018 Tom Kaitchuck
- Copyright (c) 2018 Victor Berger
- Copyright (c) 2018 Yevhenii Reizner
- Copyright (c) 2018-2019 dirs-rs contributors
- Copyright (c) 2018-2019 The RustCrypto Project Developers
- Copyright (c) 2018-2025 The rust-random Project Developers
- Copyright (c) 2018-2025 The RustCrypto Project Developers
- Copyright (c) 2018-2026 The Rand Project Developers
- Copyright (c) 2018-2026 The rust-random Project Developers
- Copyright (c) 2019 Anthony James Munns
- Copyright (c) 2019 Carl Lerche
- Copyright (c) 2019 Daniel "Lokathor" Gee.
- Copyright (c) 2019 Eliza Weisman
- Copyright (c) 2019 Fontations Developers
- Copyright (c) 2019 Ivan Molodetskikh
- Copyright (c) 2019 Kornel
- Copyright (c) 2019 Manish Goregaokar
- Copyright (c) 2019 Nick Fitzgerald
- Copyright (c) 2019 Osspial
- Copyright (c) 2019 Peter Glotfelty
- Copyright (c) 2019 Reizner Evgeniy
- Copyright (c) 2019 The Crossbeam Project Developers
- Copyright (c) 2019 Tokio Contributors
- Copyright (c) 2019 Yevhenii Reizner
- Copyright (c) 2019 Yoshua Wuyts
- Copyright (c) 2019-2026 The RustCrypto Project Developers
- Copyright (c) 2020 Andrew D. Straw
- Copyright (c) 2020 Andrey Zheleznov <zheland.net@gmail.com>
- Copyright (c) 2020 Ashish Myles and contributors
- Copyright (c) 2020 Chad Brokaw
- Copyright (c) 2020 Diggory Blake
- Copyright (c) 2020 Jared Forth
- Copyright (c) 2020 Olivier Goffart <ogoffart@sixtyfps.io>
- Copyright (c) 2020 Samuele Esposito
- Copyright (c) 2020 Yevhenii Reizner
- Copyright (c) 2020 Yoshua Wuyts
- Copyright (c) 2020 Yoshua Wuyts, Stjepan Glavina & Zeeshan Ali Khan
- Copyright (c) 2020 Élie ROUDNINSKI (marmeladema) <xademax@gmail.com>
- Copyright (c) 2020-2021 Joshua Barretto
- Copyright (c) 2020-2025 The RustCrypto Project Developers
- Copyright (c) 2020-2026 Paweł Kuna
- Copyright (c) 2020-2026 The RustCrypto Project Developers
- Copyright (c) 2021 1Password
- Copyright (c) 2021 4lDO2
- Copyright (c) 2021 RustCrypto Developers
- Copyright (c) 2021-2022 The Nushell Project Developers
- Copyright (c) 2021-2026 RustCrypto Developers
- Copyright (c) 2022 Arthur Silva
- Copyright (c) 2022 Bartłomiej Maryńczak
- Copyright (c) 2022 Ivan Smirnov
- Copyright (c) 2022 Tait Hoyem <tait@tait.tech>
- Copyright (c) 2022 The Arboard contributors
- Copyright (c) 2022 The critical-section authors
- Copyright (c) 2022 Yevhenii Reizner
- Copyright (c) 2022-2023 John Nunley
- Copyright (c) 2022-2026 The RustCrypto Project Developers
- Copyright (c) 2023 Jacob Pratt et al.
- Copyright (c) 2023 John Nunley
- Copyright (c) 2023 Kirill Chibisov
- Copyright (c) 2023 Notify Contributors
- Copyright (c) 2024 Jacob Pratt et al.
- Copyright (c) 2024 Zeeshan Ali Khan & zbus contributors
- Copyright (c) 2025 Alice Maz, Marshall Pierce
- Copyright (c) 2025 Bugen Zhao
- Copyright (c) 2025 rutrum
- Copyright (c) 2026 Zeeshan Ali Khan & zcheapstr contributors
- Copyright (c) 2026 Zeeshan Ali Khan & zruntime contributors
- Copyright (c) [2021] [Marvin Countryman]
- Copyright (c) HarfBuzz developers
- Copyright (c) HeroicKatora 2020
- Copyright (c) Individual contributors
- Copyright (c) Jacob Pratt
- Copyright (c) Jacob Pratt et al.
- Copyright (c) The Rust Project Developers
- Copyright (c) The rust-url developers
- Copyright (c) The tar-rs Project Contributors
- Copyright (c) Tokio Contributors
- Copyright (c) Ulrik Sverdrup "bluss" 2015-2023
- Copyright (c) zune-image developers
- Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC
- Copyright 2013-2014 RAD Game Tools and Valve Software
- Copyright 2015 Google Inc. All rights reserved.
- Copyright 2016 Nika Layzell
- Copyright 2017 the Resvg Authors
- Copyright 2018 Developers of the Rand project
- Copyright 2019 as-raw-xcb-connection Contributers
- Copyright 2019 x11rb Contributers
- Copyright 2020 the Parley Authors
- Copyright 2021 Developers of the femtovg project
- Copyright 2022 Kirill Chibisov
- Copyright 2023 The Fuchsia Authors
- Copyright 2024 the Parley Authors
- Copyright Mozilla Foundation
- Copyright © 2019–2026 Trevor Spiteri
- Copyright © 2021 The pdf-rs contributers.
- Copyright © 2022 Kirill Chibisov

```
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## Unicode License v3

- Copyright © 1991-2023 Unicode, Inc.
- Copyright © 2020-2024 Unicode, Inc.

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.

SPDX-License-Identifier: Unicode-3.0

—

Portions of ICU4X may have been adapted from ICU4C and/or ICU4J.
ICU 1.8.1 to ICU 57.1 © 1995-2016 International Business Machines Corporation and others.
```
