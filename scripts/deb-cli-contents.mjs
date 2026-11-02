export function hasExecutableCli(contents) {
  return contents.split(/\r?\n/).some(line => {
    const entry = line.trimEnd();
    if (!/^-[rwxStTs-]{9}\s+.*\s(?:\.\/)?usr\/bin\/workbuddy-switch-next$/.test(entry)) return false;
    return [entry[3], entry[6], entry[9]].some(permission => /[xst]/.test(permission));
  });
}
