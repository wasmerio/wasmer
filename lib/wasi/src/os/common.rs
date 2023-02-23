pub type Pid = u32;

pub fn is_mobile(user_agent: &str) -> bool {
    user_agent.contains("Android")
        || user_agent.contains("BlackBerry")
        || user_agent.contains("iPhone")
        || user_agent.contains("iPad")
        || user_agent.contains("iPod")
        || user_agent.contains("Open Mini")
        || user_agent.contains("IEMobile")
        || user_agent.contains("WPDesktop")
}

pub fn is_ssh(user_agent: &str) -> bool {
    user_agent.contains("ssh")
}
