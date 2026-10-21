use super::*;

const GTF: &str = "\
##description: a test
chr1\tHAVANA\tgene\t1000\t2000\t.\t+\t.\tgene_id \"ENSG00000001.5\"; gene_type \"protein_coding\"; gene_name \"AAA\";
chr1\tHAVANA\ttranscript\t1000\t2000\t.\t+\t.\tgene_id \"ENSG00000001.5\"; gene_name \"AAA\";
chr2\tHAVANA\tgene\t500\t900\t.\t-\t.\tgene_id \"ENSG00000002.1\"; gene_name \"BBB\";
chrX\tHAVANA\tgene\t300\t400\t.\t+\t.\tgene_id \"ENSG00000003.2\"; gene_name \"CCC\";
chrM\tHAVANA\tgene\t1\t50\t.\t+\t.\tgene_id \"ENSG00000004.1\"; gene_name \"MT-DDD\";
";

#[test]
fn a_gtf_s_genes_are_read_once_and_kept_beside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let gtf = tmp.path().join("a.gtf");
    std::fs::write(&gtf, GTF).unwrap();
    let g = genes(&gtf).unwrap();
    assert_eq!(g.len(), 4, "gene lines only");
    assert_eq!(
        g[0],
        Gene {
            id: "ENSG00000001".into(),
            name: "AAA".into(),
            chrom: "chr1".into(),
            start: 1000,
            end: 2000
        }
    );
    assert!(genes_file(&gtf).is_file());
    // Read again from the kept table, the GTF gone.
    std::fs::remove_file(&gtf).unwrap();
    assert_eq!(genes(&gtf).unwrap(), g);
}

#[test]
fn a_gtf_s_genes_are_kept_whole_with_nothing_left_beside_them() {
    let tmp = tempfile::tempdir().unwrap();
    let gtf = tmp.path().join("a.gtf");
    std::fs::write(&gtf, GTF).unwrap();
    let g = genes(&gtf).unwrap();
    let kept = std::fs::read_to_string(genes_file(&gtf)).unwrap();
    assert_eq!(kept.lines().count(), g.len());
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
}

#[test]
fn data_genes_are_placed_by_symbol_id_or_either_part() {
    let tmp = tempfile::tempdir().unwrap();
    let gtf = tmp.path().join("a.gtf");
    std::fs::write(&gtf, GTF).unwrap();
    let g = genes(&gtf).unwrap();
    let names: Vec<String> = ["aaa", "ENSG00000002.1", "ENSG00000003_CCC", "MT-DDD", "ZZZ"]
        .map(String::from)
        .to_vec();
    let genome = Genome::place(&gtf, &g, &names);
    let chroms: Vec<&str> = genome.chroms.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(chroms, ["1", "2", "X"], "the mitochondrion left out");
    assert_eq!(
        genome.chroms[1].1, 2000,
        "chr2 starts where chr1's last gene ends"
    );
    assert_eq!(
        genome.at,
        [Some(1000), Some(2500), Some(2900 + 300), None, None]
    );
    assert_eq!(genome.found(), 3);
    assert_eq!(genome.len(), 2900 + 400);
}
