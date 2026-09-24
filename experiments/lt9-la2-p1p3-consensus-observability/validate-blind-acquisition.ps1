param(
    [Parameter(Mandatory = $true)]
    [string]$AcquisitionPath
)

$ErrorActionPreference = 'Stop'
$rootPath = (Resolve-Path -LiteralPath $AcquisitionPath).Path
$receiptPath = Join-Path $rootPath 'acquisition-receipt.json'
$preRootPath = Join-Path $rootPath 'pre-review-root.json'
$privatePath = Join-Path $rootPath 'private-ledger.json'
$excludedPath = Join-Path $rootPath 'excluded-document-hashes.json'
$qcPath = Join-Path $rootPath 'pre-review-qc.json'
$distributionPath = Join-Path $rootPath 'reviewer-distribution-receipt.json'
if ((Test-Path -LiteralPath $qcPath) -or (Test-Path -LiteralPath $distributionPath)) {
    throw 'Refusing to overwrite existing pre-review validation artifacts.'
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-PhysicalSignature($Packet) {
    $contexts = [string[]]@($Packet.left_context, $Packet.right_context)
    [Array]::Sort($contexts, [StringComparer]::Ordinal)
    [ordered]@{
        lexical_pair = @($Packet.lexical_pair)
        contexts = $contexts
    } | ConvertTo-Json -Depth 5 -Compress
}

$receipt = Get-Content -Raw -LiteralPath $receiptPath | ConvertFrom-Json
$preRoot = Get-Content -Raw -LiteralPath $preRootPath | ConvertFrom-Json
$privateRows = Get-Content -Raw -LiteralPath $privatePath | ConvertFrom-Json
$excludedDocs = Get-Content -Raw -LiteralPath $excludedPath | ConvertFrom-Json
if ($receipt.status -ne 'BLIND_PACKETS_READY' -or $receipt.candidates.Count -ne 4 -or $receipt.physical_pair_count -ne 120) {
    throw 'The acquisition did not meet the frozen 4-candidate / 120-pair intake target.'
}
if (!$receipt.corpus_hashes_verified -or $receipt.prior_judgment_files_opened -or $receipt.qrels_or_queries_read -or $receipt.model_features_materialized -or $receipt.feature_fit -or $receipt.authority_updated -or $receipt.retrieval_run) {
    throw 'Acquisition receipt violates a frozen label-blind or no-fit invariant.'
}
if ($preRoot.acquisition_receipt_sha256 -ne (Get-Sha256 $receiptPath) -or $preRoot.private_ledger_sha256 -ne (Get-Sha256 $privatePath)) {
    throw 'Pre-review root does not bind the acquisition receipt/private ledger.'
}
if ($preRoot.protocol_sha256 -ne $receipt.protocol_sha256 -or $preRoot.rubric_sha256 -ne $receipt.rubric_sha256) {
    throw 'Protocol/rubric hashes disagree across the root and acquisition receipt.'
}

$reviewerSummary = @()
$physicalSignatures = @{}
$allPacketIds = @{}
$allBandCounts = @{ low = 0; middle = 0; high = 0 }
foreach ($reviewer in @('reviewer-1', 'reviewer-2', 'reviewer-3')) {
    $dir = Join-Path (Join-Path $rootPath 'blind-review') $reviewer
    $packetPath = Join-Path $dir 'packets.json'
    $templatePath = Join-Path $dir 'judgments-template.json'
    $packets = Get-Content -Raw -LiteralPath $packetPath | ConvertFrom-Json
    $template = Get-Content -Raw -LiteralPath $templatePath | ConvertFrom-Json
    if ($packets.Count -ne 120 -or $template.Count -ne 120) { throw "$reviewer packet/template count is not 120." }
    $ids = @{}
    $signatures = [System.Collections.Generic.List[string]]::new()
    foreach ($packet in $packets) {
        $names = @($packet.PSObject.Properties.Name | Sort-Object)
        if (($names -join ',') -ne 'left_context,lexical_pair,packet_id,right_context') { throw "$reviewer packet exposes unexpected fields." }
        if ($ids.ContainsKey($packet.packet_id)) { throw "$reviewer has a duplicate packet ID." }
        $ids[$packet.packet_id] = $true
        if ($allPacketIds.ContainsKey($packet.packet_id)) { throw 'Reviewer packet IDs are not independently opaque.' }
        $allPacketIds[$packet.packet_id] = $true
        $signatures.Add((Get-PhysicalSignature $packet))
    }
    foreach ($row in $template) {
        $names = @($row.PSObject.Properties.Name | Sort-Object)
        if (($names -join ',') -ne 'judgment,packet_id' -or $null -ne $row.judgment -or !$ids.ContainsKey($row.packet_id)) {
            throw "$reviewer template has a changed ID set or a nonempty label."
        }
    }
    $signatureKey = ($signatures | Sort-Object) -join "`n"
    $physicalSignatures[$reviewer] = $signatureKey
    $expected = $receipt.reviewer_files | Where-Object reviewer_slot -eq $reviewer
    if ($expected.packet_count -ne 120 -or $expected.packets_sha256 -ne (Get-Sha256 $packetPath) -or $expected.template_sha256 -ne (Get-Sha256 $templatePath)) {
        throw "$reviewer files do not match their acquisition receipt hashes."
    }
    if ((Get-Sha256 (Join-Path $dir 'rubric.md')) -ne $receipt.rubric_sha256) { throw "$reviewer rubric hash mismatch." }
    $reviewerSummary += [ordered]@{
        reviewer_slot = $reviewer
        packet_count = $packets.Count
        unique_packet_ids = $ids.Count
        packets_sha256 = Get-Sha256 $packetPath
        template_sha256 = Get-Sha256 $templatePath
    }
}
if ($physicalSignatures['reviewer-1'] -ne $physicalSignatures['reviewer-2'] -or $physicalSignatures['reviewer-1'] -ne $physicalSignatures['reviewer-3']) {
    throw 'Reviewer files do not contain the same physical context pairs.'
}

$candidateIds = @{}
foreach ($candidate in $receipt.candidates) {
    if ($candidate.status -ne 'PACKETS_READY' -or $candidate.fit_contexts -ne 6 -or $candidate.holdout_contexts -ne 6 -or $candidate.fit_corpora.Count -lt 3 -or $candidate.holdout_corpora.Count -lt 3 -or $candidate.fit_overlap_iqr -lt 0.10 -or $candidate.holdout_overlap_iqr -lt 0.10) {
        throw "Candidate $($candidate.candidate_id) does not meet the frozen graph intake contract."
    }
    $candidateIds[$candidate.candidate_id] = $true
}

$previousDocSet = @{}
foreach ($docHash in $excludedDocs) { $previousDocSet[$docHash] = $true }
$groups = @{}
$allNodes = @{}
$allDocs = @{}
$physicalKeys = @{}
foreach ($row in $privateRows) {
    if ($physicalKeys.ContainsKey($row.edge_key) -or !$candidateIds.ContainsKey($row.candidate_id)) { throw 'Private ledger has duplicate edges or unknown candidates.' }
    $physicalKeys[$row.edge_key] = $true
    if (!$allBandCounts.ContainsKey($row.overlap_band)) { throw 'Private ledger has an invalid overlap band.' }
    $allBandCounts[$row.overlap_band]++
    $key = "$($row.candidate_id)|$($row.split)"
    if (!$groups.ContainsKey($key)) { $groups[$key] = @{ edges = @{}; nodes = @{}; docs = @{} } }
    $group = $groups[$key]
    $group.edges[$row.edge_key] = $true
    foreach ($endpoint in @($row.left, $row.right)) {
        if ($previousDocSet.ContainsKey($endpoint.document_sha256)) { throw 'A selected occurrence reuses a previously reviewed document.' }
        if ($allDocs.ContainsKey($endpoint.document_sha256) -and $allDocs[$endpoint.document_sha256] -ne $endpoint.node_id) {
            throw 'A document is reused by multiple sampled occurrence nodes.'
        }
        $allDocs[$endpoint.document_sha256] = $endpoint.node_id
        $allNodes[$endpoint.node_id] = $true
        $group.nodes[$endpoint.node_id] = $true
        $group.docs[$endpoint.document_sha256] = $true
    }
    $keys = @($row.PSObject.Properties.Name | Sort-Object)
    if (($keys -join ',') -ne 'candidate_id,edge_key,left,lexical_pair,overlap_band,reviewer_packet_ids,right,split') {
        throw 'Private ledger contains unexpected feature-bearing fields.'
    }
}
if ($privateRows.Count -ne 120 -or $allPacketIds.Count -ne 360 -or $allNodes.Count -ne 48 -or $allDocs.Count -ne 48) {
    throw 'Physical edge, reviewer ID, or occurrence-document counts are inconsistent.'
}
foreach ($candidate in $receipt.candidates) {
    $fitKey = "$($candidate.candidate_id)|fit"
    $holdKey = "$($candidate.candidate_id)|holdout"
    if (!$groups.ContainsKey($fitKey) -or !$groups.ContainsKey($holdKey)) { throw 'Candidate lacks a fit or holdout graph.' }
    if ($groups[$fitKey].edges.Count -ne 15 -or $groups[$holdKey].edges.Count -ne 15 -or $groups[$fitKey].nodes.Count -ne 6 -or $groups[$holdKey].nodes.Count -ne 6) {
        throw 'Graph edge/node counts do not match the frozen 6-context / 15-edge construction.'
    }
    foreach ($docHash in $groups[$fitKey].docs.Keys) {
        if ($groups[$holdKey].docs.ContainsKey($docHash)) { throw 'Fit/holdout graphs share a document.' }
    }
}

$qc = [ordered]@{
    schema = 'phoenix.lexical.lt9-la2-p1p3-pre-review-qc/v1'
    status = 'PASS_LABEL_BLIND_ACQUISITION_QA'
    pre_review_root_sha256 = Get-Sha256 $preRootPath
    acquisition_receipt_sha256 = Get-Sha256 $receiptPath
    private_ledger_sha256 = Get-Sha256 $privatePath
    physical_pairs = $privateRows.Count
    reviewer_packet_count_each = 120
    candidates = $receipt.candidates.Count
    occurrence_nodes = $allNodes.Count
    distinct_selected_documents = $allDocs.Count
    prior_document_overlap = 0
    reviewer_sets_same_physical_pairs = $true
    fit_holdout_document_overlap = 0
    model_feature_fields_in_private_ledger = $false
    prior_review_labels_opened = $false
    model_features_materialized = $false
    feature_fit = $false
    authority_updated = $false
    retrieval_run = $false
    overlap_band_counts = $allBandCounts
    reviewer_files = $reviewerSummary
}
$qc | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -LiteralPath $qcPath

$packageRows = @()
foreach ($reviewer in @('reviewer-1', 'reviewer-2', 'reviewer-3')) {
    $zipPath = Join-Path $rootPath "$reviewer-review.zip"
    if (Test-Path -LiteralPath $zipPath) { throw "Refusing to overwrite $zipPath" }
    $dir = Join-Path (Join-Path $rootPath 'blind-review') $reviewer
    Compress-Archive -Path (Join-Path $dir '*') -DestinationPath $zipPath -CompressionLevel Optimal
    $packageRows += [ordered]@{
        reviewer_slot = $reviewer
        package_file = [IO.Path]::GetFileName($zipPath)
        package_sha256 = Get-Sha256 $zipPath
        contents = @('packets.json', 'judgments-template.json', 'rubric.md')
    }
}
[ordered]@{
    schema = 'phoenix.lexical.lt9-la2-p1p3-reviewer-distribution/v1'
    status = 'THREE_SEPARATE_BLIND_REVIEW_PACKAGES_READY'
    pre_review_root_sha256 = Get-Sha256 $preRootPath
    qc_receipt_sha256 = Get-Sha256 $qcPath
    packages = $packageRows
    review_labels_created = $false
    packets_or_templates_changed = $false
} | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 -LiteralPath $distributionPath

Write-Output "PASS: 4 candidate graphs, 120 physical pairs, three distinct-ID reviewer packages. No labels or features were read."
