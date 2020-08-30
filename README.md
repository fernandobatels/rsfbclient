# Rust Firebird Client 

![Build](https://github.com/fernandobatels/rsfbclient/workflows/testing_changes/badge.svg)
[![Crate](https://img.shields.io/crates/v/rsfbclient.svg)](https://crates.io/crates/rsfbclient)
[![API](https://docs.rs/rsfbclient/badge.svg)](https://docs.rs/rsfbclient)

Binds to official firebird client lib

## Goals 

- [x] Rust Api
- [ ] Static link with fbclient
- [x] Dynamic link with fbclient
- [x] Dynamic loading the fbclient(.dll or .so)
- [x] ARM support
- [x] Firebird embedded support
- [x] Extern this [api to ruby](https://github.com/fernandobatels/rbfbclient)
- [ ] Extern this api to lua (in a new repo)

## Firebird Reference

- https://firebirdsql.org/manual/ufb-cs-clientlib.html
- http://www.ibase.ru/files/interbase/ib6/ApiGuide.pdf
- http://www.firebirdsql.org/pdfrefdocs/Firebird-2.1-ErrorCodes.pdf
- https://www.firebirdsql.org/file/documentation/html/en/refdocs/fblangref25/firebird-25-language-reference.html#fblangref25-appx02-sqlstates

## Contributions 

A special thanks to @jairinhohw for your contributions
