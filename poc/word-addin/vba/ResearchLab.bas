Attribute VB_Name = "ResearchLab"
' Research Lab citation PoC: the Zotero-style Word macro route (Phase 0-B).
'
' Comments and literals are ASCII on purpose: the Mac VBA editor imports .bas files
' in a legacy code page and garbles CJK text. All user-facing Chinese strings are
' fetched from the bridge (docx/rl-bridge.ts, "strings" command).
'
' The macro only reads and writes Word. Citation formatting is computed by local
' Node (citeproc-js): AppleScriptTask -> ResearchLab.scpt -> docx/rl-bridge.ts.
' Data layout is shared with the docx tool and the Office.js add-in:
'   inline content control, tag "rl:cite:<id>", content locked
'   block content control, tag "rl:bibliography"
'   custom XML part, namespace urn:research-lab:citations:v1
Option Explicit

Private Const RL_NS As String = "urn:research-lab:citations:v1"
Private Const CITE_PREFIX As String = "rl:cite:"
Private Const BIB_TAG As String = "rl:bibliography"

Private gMsgs As Collection

' ---------- bridge ----------

Private Function Bridge(ByVal payload As String) As String
    Bridge = AppleScriptTask("ResearchLab.scpt", "rlBridge", payload)
End Function

Private Function BridgeLines(ByVal payload As String, ByRef ok As Boolean) As Variant
    Dim res As String
    res = Bridge(payload)
    If Left$(res, 2) = "OK" Then
        ok = True
        BridgeLines = Split(res, vbLf)
    Else
        ok = False
        MsgBox U(Mid$(res, 5)), vbExclamation, "Research Lab"
        BridgeLines = Array()
    End If
End Function

Private Function Msg(ByVal key As String) As String
    Dim lines As Variant, ok As Boolean, i As Long, f() As String
    If gMsgs Is Nothing Then
        Set gMsgs = New Collection
        lines = BridgeLines("{""cmd"":""strings""}", ok)
        If ok Then
            For i = 1 To UBound(lines)
                f = Split(lines(i), vbTab)
                If f(0) = "M" Then gMsgs.Add U(f(2)), f(1)
            Next i
        End If
    End If
    On Error Resume Next
    Msg = gMsgs(key)
    If Err.Number <> 0 Then Msg = key
    On Error GoTo 0
End Function

' JSON string literal
Private Function J(ByVal s As String) As String
    Dim c As Long
    s = Replace(s, "\", "\\")
    s = Replace(s, """", "\""")
    s = Replace(s, vbCr, "\r")
    s = Replace(s, vbLf, "\n")
    s = Replace(s, vbTab, "\t")
    For c = 1 To 31
        If InStr(s, Chr$(c)) > 0 Then s = Replace(s, Chr$(c), "\u" & Right$("000" & Hex$(c), 4))
    Next c
    J = """" & s & """"
End Function

' undo the bridge escaping: \\ \t \n \r
Private Function U(ByVal s As String) As String
    s = Replace(s, "\\", ChrW$(1))
    s = Replace(s, "\t", vbTab)
    s = Replace(s, "\n", vbLf)
    s = Replace(s, "\r", vbCr)
    U = Replace(s, ChrW$(1), "\")
End Function

' ---------- document access ----------

Private Function StoreXml() As String
    Dim parts As CustomXMLParts
    Set parts = ActiveDocument.CustomXMLParts.SelectByNamespace(RL_NS)
    If parts.Count > 0 Then StoreXml = parts(1).XML
End Function

Private Sub SaveStore(ByVal xml As String)
    Dim parts As CustomXMLParts, i As Long
    Set parts = ActiveDocument.CustomXMLParts.SelectByNamespace(RL_NS)
    For i = parts.Count To 1 Step -1
        parts(i).Delete
    Next i
    ActiveDocument.CustomXMLParts.Add xml
End Sub

Private Function Citations() As Collection
    Dim c As New Collection, cc As ContentControl
    For Each cc In ActiveDocument.ContentControls
        If Left$(cc.Tag, Len(CITE_PREFIX)) = CITE_PREFIX Then c.Add cc
    Next cc
    Set Citations = c
End Function

Private Function BibControl() As ContentControl
    Dim cc As ContentControl
    For Each cc In ActiveDocument.ContentControls
        If cc.Tag = BIB_TAG Then
            Set BibControl = cc
            Exit Function
        End If
    Next cc
End Function

Private Function CurrentCitation() As ContentControl
    Dim cc As ContentControl
    On Error Resume Next
    Set cc = Selection.Range.ParentContentControl
    On Error GoTo 0
    If Not cc Is Nothing Then
        If Left$(cc.Tag, Len(CITE_PREFIX)) = CITE_PREFIX Then Set CurrentCitation = cc
    End If
End Function

' fmt: "len:flags,len:flags" ; flags i b p(sup) d(sub) c(small caps) u
Private Sub ApplyFormat(ByVal startPos As Long, ByVal fmt As String)
    Dim segs() As String, i As Long, p As Long, n As Long, f As String, r As Range, pos As Long
    If fmt = "" Then Exit Sub
    pos = startPos
    segs = Split(fmt, ",")
    For i = 0 To UBound(segs)
        p = InStr(segs(i), ":")
        n = CLng(Left$(segs(i), p - 1))
        f = Mid$(segs(i), p + 1)
        If n > 0 Then
            Set r = ActiveDocument.Range(pos, pos + n)
            r.Font.Italic = (InStr(f, "i") > 0)
            r.Font.Bold = (InStr(f, "b") > 0)
            r.Font.Superscript = (InStr(f, "p") > 0)
            If InStr(f, "d") > 0 Then r.Font.Subscript = True
            r.Font.SmallCaps = (InStr(f, "c") > 0)
            If InStr(f, "u") > 0 Then
                r.Font.Underline = wdUnderlineSingle
            Else
                r.Font.Underline = wdUnderlineNone
            End If
        End If
        pos = pos + n
    Next i
End Sub

Private Sub WriteControl(cc As ContentControl, ByVal txt As String, ByVal fmt As String)
    cc.LockContents = False
    cc.Range.Text = txt
    ApplyFormat cc.Range.Start, fmt
    cc.LockContents = True
End Sub

Private Sub WriteBibliography(cc As ContentControl, texts As Collection, fmts As Collection, ByVal hang As Single)
    Dim i As Long, joined As String, p As Range
    cc.LockContents = False
    For i = 1 To texts.Count
        If i > 1 Then joined = joined & vbCr
        joined = joined & texts(i)
    Next i
    cc.Range.Text = joined
    For i = 1 To texts.Count
        Set p = cc.Range.Paragraphs(i).Range
        ApplyFormat p.Start, fmts(i)
        p.ParagraphFormat.LeftIndent = hang
        p.ParagraphFormat.FirstLineIndent = -hang
    Next i
    cc.LockContents = True
End Sub

Private Function NewId() As String
    Randomize
    NewId = LCase$(Hex$(CLng(Rnd * 2147483647#))) & Format$(Now, "yymmddhhnnss")
End Function

' ---------- refresh ----------

Private Sub DoRefresh(ByVal style As String, ByVal addJson As String)
    Dim ccs As Collection, payload As String, lines As Variant, ok As Boolean, i As Long, f() As String
    Dim tags As String, texts As New Collection, fmts As New Collection, hang As Single
    Dim newStore As String, notes As String, bib As ContentControl, cc As ContentControl, n As Long

    Set ccs = Citations()
    tags = "["
    For i = 1 To ccs.Count
        If i > 1 Then tags = tags & ","
        tags = tags & J(ccs(i).Tag)
    Next i
    tags = tags & "]"
    If addJson = "" Then addJson = "null"
    payload = "{""cmd"":""refresh"",""style"":" & J(style) & ",""add"":" & addJson & _
              ",""tags"":" & tags & ",""store"":" & J(StoreXml()) & "}"
    lines = BridgeLines(payload, ok)
    If Not ok Then Exit Sub

    For i = 1 To UBound(lines)
        f = Split(lines(i), vbTab)
        Select Case f(0)
            Case "C"
                Set cc = ccs(CLng(f(1)) + 1)
                If f(2) <> "" Then cc.Tag = f(2)
                WriteControl cc, U(f(3)), f(4)
                n = n + 1
            Case "B"
                hang = CSng(Val(f(1)))
            Case "E"
                texts.Add U(f(1))
                fmts.Add f(2)
            Case "X", "W"
                notes = notes & U(f(UBound(f))) & vbLf
            Case "S"
                newStore = U(f(1))
        End Select
    Next i
    Set bib = BibControl()
    If Not bib Is Nothing Then WriteBibliography bib, texts, fmts, hang
    If newStore <> "" Then SaveStore newStore
    If notes <> "" Then MsgBox notes, vbInformation, "Research Lab"
    On Error Resume Next
    Application.StatusBar = Msg("done") & " (" & n & ")"
End Sub

' ---------- public macros ----------

Public Sub RL_Refresh()
    DoRefresh "", ""
End Sub

Public Sub RL_SwitchStyle()
    Dim lines As Variant, ok As Boolean, i As Long, f() As String, ids As New Collection, menu As String, pick As String
    lines = BridgeLines("{""cmd"":""styles""}", ok)
    If Not ok Then Exit Sub
    For i = 1 To UBound(lines)
        f = Split(lines(i), vbTab)
        If f(0) = "Y" Then
            ids.Add f(1)
            menu = menu & ids.Count & "  " & U(f(2)) & vbLf
        End If
    Next i
    pick = Trim$(InputBox(Msg("style") & vbLf & menu, "Research Lab"))
    If pick = "" Then Exit Sub
    If Val(pick) < 1 Or Val(pick) > ids.Count Then
        MsgBox Msg("badpick") & pick, vbExclamation, "Research Lab"
        Exit Sub
    End If
    DoRefresh ids(CLng(Val(pick))), ""
End Sub

Public Sub RL_InsertCitation()
    Dim q As String, lines As Variant, ok As Boolean, i As Long, f() As String, ids As New Collection
    Dim menu As String, pick As String, tokens() As String, tok As String, parts() As String
    Dim items As String, k As Long, cid As String, rng As Range, cc As ContentControl

    If Not CurrentCitation() Is Nothing Then
        MsgBox Msg("incite"), vbExclamation, "Research Lab"
        Exit Sub
    End If
    q = InputBox(Msg("search"), "Research Lab")
    lines = BridgeLines("{""cmd"":""search"",""q"":" & J(q) & "}", ok)
    If Not ok Then Exit Sub
    For i = 1 To UBound(lines)
        f = Split(lines(i), vbTab)
        If f(0) = "I" Then
            ids.Add f(1)
            menu = menu & ids.Count & "  " & U(f(2)) & vbLf
        End If
    Next i
    If ids.Count = 0 Then
        MsgBox Msg("nohit"), vbInformation, "Research Lab"
        Exit Sub
    End If
    pick = Trim$(InputBox(Msg("pick") & vbLf & menu, "Research Lab"))
    If pick = "" Then Exit Sub

    items = "["
    tokens = Split(pick, ",")
    For i = 0 To UBound(tokens)
        tok = Trim$(tokens(i))
        If tok <> "" Then
            parts = Split(tok, ":")
            k = CLng(Val(parts(0)))
            If k < 1 Or k > ids.Count Then
                MsgBox Msg("badpick") & tok, vbExclamation, "Research Lab"
                Exit Sub
            End If
            If Len(items) > 1 Then items = items & ","
            items = items & "{""id"":" & J(ids(k))
            If UBound(parts) >= 1 Then
                If Trim$(parts(1)) <> "" Then items = items & ",""locator"":" & J(Trim$(parts(1)))
            End If
            items = items & "}"
        End If
    Next i
    items = items & "]"

    cid = NewId()
    Set rng = Selection.Range
    rng.Collapse wdCollapseEnd
    Set cc = ActiveDocument.ContentControls.Add(wdContentControlRichText, rng)
    cc.Tag = CITE_PREFIX & cid
    cc.Title = "Research Lab"
    cc.Range.Text = ChrW$(&H2026)
    DoRefresh "", "{""citationID"":" & J(cid) & ",""items"":" & items & "}"
    Selection.SetRange cc.Range.End + 1, cc.Range.End + 1
End Sub

Public Sub RL_InsertBibliography()
    Dim rng As Range, cc As ContentControl
    If Not BibControl() Is Nothing Then
        MsgBox Msg("hasbib"), vbInformation, "Research Lab"
        Exit Sub
    End If
    Set rng = Selection.Paragraphs(1).Range
    rng.InsertParagraphAfter
    Set rng = rng.Next(wdParagraph, 1)
    Set cc = ActiveDocument.ContentControls.Add(wdContentControlRichText, rng)
    cc.Tag = BIB_TAG
    cc.Title = "Research Lab"
    DoRefresh "", ""
End Sub
