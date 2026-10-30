// Fault injection only: accept ports but never complete a request.
globalThis.onmessage = ({ data }) => {
    if (data.port) { data.port.onmessage = () => {}; data.port.start(); }
};
