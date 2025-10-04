For the bluetooth communication, switch to a command/result model, with two characteristics, one WRITE characteristic where the central writes the command, and, if the device returned success to the write, a READ characteristic where the result of the command can be read from.

We only allow a single client at a time, so should be trivial to implement.

There is also a third characteristic that allows reading recent log entries and errors.

The fixed operations (premult, noise-gate, etc) could be replaced by an array of "Operations".