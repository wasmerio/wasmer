export function showCanvas() {
  document.getElementById("terminal").style.display='none';
  document.getElementById("frontBuffer").style.display='inline';
}

export function showTerminal() {
  document.getElementById("terminal").style.display='inline';
  document.getElementById("frontBuffer").style.display='none';
}