-- Pith Word bridge. Installed by Pith; do not edit (it is overwritten on update).
-- The Pith Word template calls this with AppleScriptTask. The request is written to a
-- temporary file (it can be large) and handed to the helper script, which talks to the
-- local Pith service.
on pithCall(payload)
	set helper to "@HELPER@"
	try
		set reqPath to (POSIX path of (path to temporary items from user domain)) & "pith-word-" & (do shell script "/usr/bin/uuidgen") & ".json"
		set fh to open for access (POSIX file reqPath) with write permission
		try
			set eof of fh to 0
			write payload to fh as «class utf8»
			close access fh
		on error errMsg
			close access fh
			error errMsg
		end try
	on error errMsg
		return "ERR" & tab & "Pith could not prepare the request: " & errMsg
	end try
	try
		set out to do shell script "/bin/sh " & quoted form of helper & " " & quoted form of reqPath without altering line endings
	on error errMsg
		set out to "ERR" & tab & "The Pith bridge failed: " & errMsg
	end try
	try
		do shell script "/bin/rm -f " & quoted form of reqPath
	end try
	return out
end pithCall
