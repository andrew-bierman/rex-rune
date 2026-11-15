function colored(mode, inFront, index) {
    return mode === "always" || (mode === "active_tab" && inFront) || (mode === "active_pane" && inFront && index === 0)
}
