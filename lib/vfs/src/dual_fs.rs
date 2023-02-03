//! DualFs is a filesystem that can use both Memory and HostFs for reading,
//! but only Memory for writing to files (so that files can be read if the
//! directory mappings are set up correctly, but files can't accidentally be 
//! written to the host machine).
