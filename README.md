# Giga Grabber
A very fast and stable [Mega](https://mega.nz) downloader built using a modified version of [mega-rs](https://github.com/Hirevo/mega-rs).
It supports multiple concurrent downloads and multiple threads per download. It can resume incomplete downloads even in the event of a crash.
Giga Grabber is fully cross platform, but does require at least OpenGL to render it's GUI.

#### Giga Grabber v0.1.0
The original CLI version of Giga Grabber

#### Giga Grabber v1.1.0
An updated GUI version of Giga Grabber with improved performance, stability, configurability, and usability.

### v0.1.0 (CLI) Usage
```
./giga_grabber --help
./giga_grabber --url <mega folder URL>
```

### v1.1.0 (GUI) Usage
```
./giga_grabber
```

# Contributing
All contributions are welcome. I would especially appreciate help with the following:
- [ ] Light / Dark theme improvements. Currently, there is no Light theme, even though it is supported.
- [ ] UI improvements. I am not a very good UI designer, so any improvements are welcome.
- [ ] UX improvements. I think it is pretty solid already, but outside opinions are always welcome.

# Warnings
- Using proxies or a VPN to bypass Mega's download limit is a violation of their [Terms of Service](https://mega.nz/terms)
- Giga Grabber can use a massive amount of bandwidth. Carefully consider the options you use.

[Learn more](https://chanchan.dev/giga-grabber)
