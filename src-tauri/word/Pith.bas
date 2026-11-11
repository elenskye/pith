Attribute VB_Name = "PithMain"
'' Pith for Word (v0.2 W2). Installed by Pith as the global template Pith.dotm;
' do not edit (it is rebuilt from this file on every install).
'
' Source is ASCII only: the Mac VBA editor reads code in a legacy code page.
' Everything shown in Word is English (Pith decision 0006).
'
'' The macro only reads and writes Word. Citation text is computed by Pith:
'   AppleScriptTask -> Pith.scpt -> Pith-bridge.sh -> local Pith service (POST /v1/word).
' Choosing references happens in Pith's citation window (Pith brings Word back to the front).
' Data layout, shared with Pith's engine that edits .docx files directly:
'   inline content control, tag "pith:cite:<id>", content locked
'   block content control, tag "pith:bibliography"
'   custom XML part, namespace urn:pith:citations:v1 (JSON inside)
' The Phase 0-B names (rl:cite:, rl:bibliography, urn:research-lab:citations:v1) are read too
' and replaced on refresh.
'
' Office library types (IRibbonControl, CustomXMLParts) are used through Object (late binding):
' the generated VBA project has no references.
Option Explicit

Private Const PITH_NS As String = "urn:pith:citations:v1"
Private Const OLD_NS As String = "urn:research-lab:citations:v1"
Private Const CITE_PREFIX As String = "pith:cite:"
Private Const OLD_CITE_PREFIX As String = "rl:cite:"
Private Const BIB_TAG As String = "pith:bibliography"
Private Const OLD_BIB_TAG As String = "rl:bibliography"
Private Const APP_TITLE As String = "Pith"

' ---------- ribbon ----------

Public Sub PithRibbonAction(ByVal control As Object)
    Select Case control.ID
        Case "PithAddCitation"
            PithAddEditCitation
        Case "PithAddBibliography"
            PithAddBibliography
        Case "PithRefresh"
            PithRefresh
        Case "PithFromZotero"
            PithFromZotero
        Case "PithOpen"
            PithOpen
    End Select
End Sub

' dynamicMenu getContent: the styles Pith knows (built-in and imported), the current one ticked.
Public Sub PithStyleMenu(ByVal control As Object, ByRef content As Variant)
    Dim lines As Variant, ok As Boolean, i As Long, f() As String, menuXml As String, lbl As String
    menuXml = "<menu xmlns=""http://schemas.microsoft.com/office/2006/01/customui"">"
    If Documents.Count = 0 Then
        menuXml = menuXml & "<button id=""PithStyleNoDoc"" label=""Open a document first"" enabled=""false""/>"
    Else
        lines = BridgeLines("{""cmd"":""styles"",""store"":" & J(StoreXml()) & "}", ok)
        If ok Then
            For i = 1 To UBound(lines)
                f = Split(lines(i), vbTab)
                If f(0) = "Y" Then
                    lbl = U(f(2))
                    If f(3) = "1" Then
                        lbl = ChrW$(&H2713) & " " & lbl
                    Else
                        lbl = "     " & lbl
                    End If
                    menuXml = menuXml & "<button id=""PithStyle" & i & """ label=""" & XmlAttr(lbl) & """ tag=""" & XmlAttr(U(f(1))) & """ onAction=""PithMain.PithStyleChosen""/>"
                End If
            Next i
        Else
            menuXml = menuXml & "<button id=""PithStyleNone"" label=""Pith is not available"" enabled=""false""/>"
        End If
    End If
    content = menuXml & "</menu>"
End Sub

Public Sub PithStyleChosen(ByVal control As Object)
    If Not HasDocument() Then Exit Sub
    If DoRefresh(control.Tag, "") Then
        On Error Resume Next
        Application.StatusBar = "Pith: citation style changed"
    End If
End Sub

' ---------- bridge ----------

Private Function Bridge(ByVal payload As String) As String
    On Error GoTo NoBridge
    Bridge = AppleScriptTask("Pith.scpt", "pithCall", payload)
    Exit Function
NoBridge:
    Bridge = "ERR" & vbTab & "The Pith bridge is not installed. In Pith, open Settings > Word and click Install."
End Function

' Returns the reply lines; shows the error and returns ok = False when Pith says ERR.
Private Function BridgeLines(ByVal payload As String, ByRef ok As Boolean) As Variant
    Dim res As String
    res = Bridge(payload)
    If Left$(res, 2) = "OK" Then
        ok = True
        BridgeLines = Split(res, vbLf)
    Else
        ok = False
        If Left$(res, 4) = "ERR" & vbTab Then res = Mid$(res, 5)
        If res = "" Then res = "Pith did not answer."
        MsgBox U(res), vbExclamation, APP_TITLE
        BridgeLines = Array()
    End If
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

'' XML attribute value
Private Function XmlAttr(ByVal s As String) As String
    s = Replace(s, "&", "&amp;")
    s = Replace(s, "<", "&lt;")
    s = Replace(s, ">", "&gt;")
    XmlAttr = Replace(s, """", "&quot;")
End Function

' Undo the reply escaping: \\ \t \n \r
Private Function U(ByVal s As String) As String
    s = Replace(s, "\\", ChrW$(1))
    s = Replace(s, "\t", vbTab)
    s = Replace(s, "\n", vbLf)
    s = Replace(s, "\r", vbCr)
    U = Replace(s, ChrW$(1), "\")
End Function

' ---------- document access ----------

Private Function HasDocument() As Boolean
    If Documents.Count = 0 Then
        MsgBox "Open a document first.", vbInformation, APP_TITLE
        Exit Function
    End If
    HasDocument = True
End Function

Private Function StoreXml() As String
    Dim parts As Object
    Set parts = ActiveDocument.CustomXMLParts.SelectByNamespace(PITH_NS)
    If parts.Count = 0 Then Set parts = ActiveDocument.CustomXMLParts.SelectByNamespace(OLD_NS)
    If parts.Count > 0 Then StoreXml = parts(1).XML
End Function

Private Sub DeleteParts(ByVal ns As String)
    Dim parts As Object, i As Long
    Set parts = ActiveDocument.CustomXMLParts.SelectByNamespace(ns)
    For i = parts.Count To 1 Step -1
        parts(i).Delete
    Next i
End Sub

Private Sub SaveStore(ByVal xml As String)
    DeleteParts PITH_NS
    DeleteParts OLD_NS
    ActiveDocument.CustomXMLParts.Add xml
End Sub

Private Function IsCitationTag(ByVal t As String) As Boolean
    IsCitationTag = (Left$(t, Len(CITE_PREFIX)) = CITE_PREFIX) Or (Left$(t, Len(OLD_CITE_PREFIX)) = OLD_CITE_PREFIX)
End Function

Private Function CitationIdOf(ByVal t As String) As String
    If Left$(t, Len(CITE_PREFIX)) = CITE_PREFIX Then
        CitationIdOf = Mid$(t, Len(CITE_PREFIX) + 1)
    ElseIf Left$(t, Len(OLD_CITE_PREFIX)) = OLD_CITE_PREFIX Then
        CitationIdOf = Mid$(t, Len(OLD_CITE_PREFIX) + 1)
    End If
End Function

Private Function IsBibliographyTag(ByVal t As String) As Boolean
    IsBibliographyTag = (t = BIB_TAG) Or (t = OLD_BIB_TAG)
End Function

Private Function Citations() As Collection
    Dim c As New Collection, cc As ContentControl
    For Each cc In ActiveDocument.ContentControls
        If IsCitationTag(cc.Tag) Then c.Add cc
    Next cc
    Set Citations = c
End Function

Private Function Bibliographies() As Collection
    Dim c As New Collection, cc As ContentControl
    For Each cc In ActiveDocument.ContentControls
        If IsBibliographyTag(cc.Tag) Then c.Add cc
    Next cc
    Set Bibliographies = c
End Function

' The Pith citation at the cursor: the cursor is inside it, or the selection covers exactly one
' (clicking the control's title selects the whole control).
Private Function CurrentCitation() As ContentControl
    Dim cc As ContentControl, r As Range
    On Error Resume Next
    Set r = Selection.Range
    Set cc = r.ParentContentControl
    If cc Is Nothing Then
        If r.ContentControls.Count = 1 Then Set cc = r.ContentControls(1)
    End If
    On Error GoTo 0
    If Not cc Is Nothing Then
        If IsCitationTag(cc.Tag) Then Set CurrentCitation = cc
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
    If cc.Tag <> BIB_TAG Then cc.Tag = BIB_TAG
    cc.LockContents = True
End Sub

Private Function NewId() As String
    Randomize
    NewId = LCase$(Hex$(CLng(Rnd * 2147483647#))) & LCase$(Hex$(CLng(Timer * 1000))) & Format$(Now, "yymmddhhnnss")
End Function

' ---------- refresh ----------

' Sends the citation tags and the document data to Pith, writes back what Pith computed.
' addJson: the citation being inserted ({"citationID":...,"items":[...]}) or "".
Private Function DoRefresh(ByVal style As String, ByVal addJson As String) As Boolean
    Dim ccs As Collection, bibs As Collection, payload As String, lines As Variant, ok As Boolean, i As Long, f() As String
    Dim tags As String, texts As New Collection, fmts As New Collection, hang As Single
    Dim newStore As String, notes As String, cc As ContentControl, n As Long

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
    If Not ok Then Exit Function

    On Error GoTo Failed
    Application.ScreenUpdating = False
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
            Case "N"
                notes = notes & U(f(1)) & vbLf
            Case "S"
                newStore = U(f(1))
        End Select
    Next i
    Set bibs = Bibliographies()
    For i = 1 To bibs.Count
        WriteBibliography bibs(i), texts, fmts, hang
    Next i
    If newStore <> "" Then SaveStore newStore
    Application.ScreenUpdating = True
    On Error GoTo 0
    If notes <> "" Then MsgBox notes, vbInformation, APP_TITLE
    On Error Resume Next
    Application.StatusBar = "Pith: " & n & " citation(s) updated"
    DoRefresh = True
    Exit Function
Failed:
    Application.ScreenUpdating = True
    MsgBox "Pith could not write to this document: " & Err.Description, vbExclamation, APP_TITLE
End Function

' ---------- commands ----------

Public Sub PithRefresh()
    If Not HasDocument() Then Exit Sub
    DoRefresh "", ""
End Sub

' Add/Edit Citation. Pith opens its citation window and waits until the user is done.
' Cursor outside a citation: insert a new one. Inside: change its references and pages, or remove it.
Public Sub PithAddEditCitation()
    Dim lines As Variant, ok As Boolean, i As Long, f() As String, items As String, action As String
    Dim cc As ContentControl, cid As String

    If Not HasDocument() Then Exit Sub
    Set cc = CurrentCitation()
    If Not cc Is Nothing Then cid = CitationIdOf(cc.Tag)
    lines = BridgeLines("{""cmd"":""pick"",""edit"":" & J(cid) & ",""store"":" & J(StoreXml()) & "}", ok)
    If Not ok Then Exit Sub
    For i = 1 To UBound(lines)
        f = Split(lines(i), vbTab)
        Select Case f(0)
            Case "P"
                items = U(f(1))
                action = "P"
            Case "D", "X"
                action = f(0)
        End Select
    Next i
    Select Case action
        Case "P"
            If cc Is Nothing Then
                InsertCitation items
            Else
                DoRefresh "", "{""citationID"":" & J(cid) & ",""items"":" & items & "}"
            End If
        Case "D"
            If Not cc Is Nothing Then
                cc.LockContents = False
                cc.Delete True
                DoRefresh "", ""
            End If
    End Select
End Sub

Private Sub InsertCitation(ByVal items As String)
    Dim cid As String, rng As Range, cc As ContentControl
    cid = NewId()
    Set rng = Selection.Range
    rng.Collapse wdCollapseEnd
    Set cc = ActiveDocument.ContentControls.Add(wdContentControlRichText, rng)
    cc.Tag = CITE_PREFIX & cid
    cc.Title = APP_TITLE
    cc.Range.Text = ChrW$(&H2026)
    If Not DoRefresh("", "{""citationID"":" & J(cid) & ",""items"":" & items & "}") Then
        cc.Delete True
        Exit Sub
    End If
    Selection.SetRange cc.Range.End + 1, cc.Range.End + 1
End Sub

' ---------- Zotero ----------

' Zotero keeps its document preferences in custom document properties ZOTERO_PREF_1, _2, ...
Private Function ZoteroPrefs() As String
    Dim props As Object, i As Long, s As String
    On Error Resume Next
    Set props = ActiveDocument.CustomDocumentProperties
    For i = 1 To 50
        s = ""
        Err.Clear
        s = props("ZOTERO_PREF_" & i).Value
        If Err.Number <> 0 Then Exit For
        ZoteroPrefs = ZoteroPrefs & s
    Next i
End Function

' A Zotero citation field becomes a Pith citation control around the same text.
Private Sub ZoteroToControl(ByVal fld As Field, ByVal cid As String)
    Dim startPos As Long, n As Long, cc As ContentControl
    n = fld.Result.End - fld.Result.Start
    startPos = fld.Code.Start - 1
    fld.Unlink
    Set cc = ActiveDocument.ContentControls.Add(wdContentControlRichText, ActiveDocument.Range(startPos, startPos + n))
    cc.Tag = CITE_PREFIX & cid
    cc.Title = APP_TITLE
    cc.LockContents = True
End Sub

' The Zotero bibliography field becomes an empty paragraph wrapped in the Pith bibliography control
' (Pith fills it on refresh). Whatever followed the field in its last paragraph stays after it.
Private Sub ZoteroBibToControl(ByVal fld As Field)
    Dim startPos As Long, n As Long, para As Range, cc As ContentControl
    n = fld.Result.End - fld.Result.Start
    startPos = fld.Code.Start - 1
    fld.Unlink
    ActiveDocument.Range(startPos, startPos + n).Delete
    ActiveDocument.Range(startPos, startPos).InsertAfter vbCr
    Set para = ActiveDocument.Range(startPos, startPos).Paragraphs(1).Range
    Set cc = ActiveDocument.ContentControls.Add(wdContentControlRichText, para)
    cc.Tag = BIB_TAG
    cc.Title = APP_TITLE
End Sub

' Convert Zotero: Pith maps each Zotero citation to a Pith citation (by Zotero item key; references
' not in the Pith library keep the data stored in the document), then the fields become controls.
Public Sub PithFromZotero()
    Dim flds As New Collection, fld As Field, bib As Field, code As String, codes As String, i As Long
    Dim lines As Variant, ok As Boolean, f() As String, ids() As String, summary As String, newStore As String, msg As String

    If Not HasDocument() Then Exit Sub
    For Each fld In ActiveDocument.Fields
        code = fld.Code.Text
        If InStr(1, code, "ADDIN ZOTERO_ITEM", vbBinaryCompare) > 0 Then
            flds.Add fld
        ElseIf InStr(1, code, "ADDIN ZOTERO_BIBL", vbBinaryCompare) > 0 Then
            If bib Is Nothing Then Set bib = fld
        End If
    Next fld
    If flds.Count = 0 And bib Is Nothing Then
        MsgBox "No Zotero citations were found in the body of this document.", vbInformation, APP_TITLE
        Exit Sub
    End If
    msg = "Convert " & flds.Count & " Zotero citation(s)"
    If Not bib Is Nothing Then msg = msg & " and the Zotero bibliography"
    msg = msg & " to Pith citations?" & vbLf & vbLf & "Zotero will no longer update them. If you may need the Zotero version again, " & _
          "save a copy of this document first. Edit > Undo can also take the change back."
    If MsgBox(msg, vbOKCancel + vbQuestion, APP_TITLE) <> vbOK Then Exit Sub

    codes = "["
    For i = 1 To flds.Count
        If i > 1 Then codes = codes & ","
        codes = codes & J(flds(i).Code.Text)
    Next i
    codes = codes & "]"
    lines = BridgeLines("{""cmd"":""zotero"",""codes"":" & codes & ",""prefs"":" & J(ZoteroPrefs()) & ",""store"":" & J(StoreXml()) & "}", ok)
    If Not ok Then Exit Sub
    If flds.Count > 0 Then ReDim ids(1 To flds.Count)
    For i = 1 To UBound(lines)
        f = Split(lines(i), vbTab)
        Select Case f(0)
            Case "Z"
                ids(CLng(f(1)) + 1) = U(f(2))
            Case "M"
                summary = U(f(1))
            Case "S"
                newStore = U(f(1))
        End Select
    Next i
    If newStore = "" Then Exit Sub

    On Error GoTo Failed
    Application.ScreenUpdating = False
    ' back to front, so that converting one field does not move the ones before it
    For i = flds.Count To 1 Step -1
        If ids(i) <> "" Then ZoteroToControl flds(i), ids(i)
    Next i
    If Not bib Is Nothing Then
        If Bibliographies().Count = 0 Then ZoteroBibToControl bib
    End If
    SaveStore newStore
    Application.ScreenUpdating = True
    On Error GoTo 0
    If DoRefresh("", "") Then MsgBox summary, vbInformation, APP_TITLE
    Exit Sub
Failed:
    Application.ScreenUpdating = True
    MsgBox "Pith could not convert this document: " & Err.Description, vbExclamation, APP_TITLE
End Sub

' Inserts the bibliography as a block content control below the current paragraph.
' Two paragraphs are added so that the control never wraps the document's last
' paragraph mark (works at the end of a document too).
Public Sub PithAddBibliography()
    Dim para As Range, rng As Range, cc As ContentControl
    If Not HasDocument() Then Exit Sub
    If Bibliographies().Count > 0 Then
        MsgBox "This document already has a Pith bibliography. Click Refresh to update it.", vbInformation, APP_TITLE
        Exit Sub
    End If
    Set para = Selection.Paragraphs(1).Range
    para.InsertParagraphAfter
    para.InsertParagraphAfter
    Set rng = para.Paragraphs(para.Paragraphs.Count - 1).Range
    Set cc = ActiveDocument.ContentControls.Add(wdContentControlRichText, rng)
    cc.Tag = BIB_TAG
    cc.Title = APP_TITLE
    DoRefresh "", ""
End Sub

Public Sub PithOpen()
    Dim lines As Variant, ok As Boolean
    lines = BridgeLines("{""cmd"":""show""}", ok)
End Sub
