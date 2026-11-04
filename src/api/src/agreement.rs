//! 標注者之間的一致性。
//!
//! - Fleiss' kappa：單選欄位，所有被標兩次以上的資料一起算，不需要知道是誰標的。
//!   每筆資料的標注人數可以不同（每筆資料的一致比例各自算，再取平均）。
//! - Cohen's kappa：兩位具名標注者都標過的資料。
//! - Krippendorff's alpha（interval）：分數欄位，允許缺值、人數不固定。
//!
//! kappa 和 alpha 都是「扣掉碰巧一致之後」的一致程度：1 是完全一致，0 和亂猜差不多，負的比亂猜還差。
//! 所有人都選同一個答案時，碰巧一致的機率是 1，公式沒有定義，回傳 None。

/// `counts[i][j]`：第 i 筆資料有幾個人選了第 j 個類別。只用標注人數至少 2 的資料。
pub fn fleiss_kappa(counts: &[Vec<usize>]) -> Option<f64> {
    let rows: Vec<&Vec<usize>> = counts.iter().filter(|r| r.iter().sum::<usize>() >= 2).collect();
    if rows.is_empty() {
        return None;
    }
    let k = rows[0].len();
    let mut category_totals = vec![0usize; k];
    let mut total = 0usize;
    let mut p_bar = 0.0;
    for row in &rows {
        let n: usize = row.iter().sum();
        let agree: usize = row.iter().map(|&c| c * c.saturating_sub(1)).sum();
        p_bar += agree as f64 / (n * (n - 1)) as f64;
        for (j, &c) in row.iter().enumerate() {
            category_totals[j] += c;
        }
        total += n;
    }
    p_bar /= rows.len() as f64;
    let p_e: f64 = category_totals.iter().map(|&c| (c as f64 / total as f64).powi(2)).sum();
    ratio(p_bar - p_e, 1.0 - p_e)
}

/// `pairs`：兩位標注者在同一筆資料上選的類別編號（0..k）。
pub fn cohen_kappa(pairs: &[(usize, usize)], k: usize) -> Option<f64> {
    if pairs.is_empty() {
        return None;
    }
    let n = pairs.len() as f64;
    let mut a = vec![0usize; k];
    let mut b = vec![0usize; k];
    let mut same = 0usize;
    for &(x, y) in pairs {
        a[x] += 1;
        b[y] += 1;
        if x == y {
            same += 1;
        }
    }
    let p_o = same as f64 / n;
    let p_e: f64 = (0..k).map(|j| (a[j] as f64 / n) * (b[j] as f64 / n)).sum();
    ratio(p_o - p_e, 1.0 - p_e)
}

/// 兩人或多人選一樣答案的比例（不扣掉碰巧一致）。`pairs` 同 [`cohen_kappa`]。
pub fn percent_agreement(pairs: &[(usize, usize)]) -> Option<f64> {
    (!pairs.is_empty()).then(|| pairs.iter().filter(|(a, b)| a == b).count() as f64 / pairs.len() as f64)
}

/// `units[u]`：第 u 筆資料收到的所有分數。只用至少有兩個分數的資料。
pub fn krippendorff_alpha_interval(units: &[Vec<f64>]) -> Option<f64> {
    let units: Vec<&Vec<f64>> = units.iter().filter(|u| u.len() >= 2).collect();
    let all: Vec<f64> = units.iter().flat_map(|u| u.iter().copied()).collect();
    let n = all.len() as f64;
    if units.is_empty() || n < 2.0 {
        return None;
    }
    // 觀察到的不一致：同一筆資料裡兩兩差的平方，依人數加權
    let mut d_o = 0.0;
    for u in &units {
        let m = u.len() as f64;
        let mut s = 0.0;
        for (i, a) in u.iter().enumerate() {
            for (j, b) in u.iter().enumerate() {
                if i != j {
                    s += (a - b).powi(2);
                }
            }
        }
        d_o += s / (m - 1.0);
    }
    d_o /= n;
    // 預期的不一致：所有分數兩兩差的平方
    let mean = all.iter().sum::<f64>() / n;
    let sum_sq: f64 = all.iter().map(|x| (x - mean).powi(2)).sum();
    // Σ_{i≠j} (x_i − x_j)² = 2n·Σ(x − mean)²
    let d_e = 2.0 * n * sum_sq / (n * (n - 1.0));
    if d_e == 0.0 {
        return None;
    }
    Some(1.0 - d_o / d_e)
}

fn ratio(num: f64, den: f64) -> Option<f64> {
    (den.abs() > 1e-12).then(|| num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Option<f64>, b: f64) {
        let a = a.expect("expected a value");
        assert!((a - b).abs() < 5e-4, "{a} != {b}");
    }

    /// Fleiss (1971) 的例子，也是 Wikipedia「Fleiss' kappa」條目的表格：10 筆資料、每筆 14 人、5 個類別，κ ≈ 0.210。
    #[test]
    fn fleiss_matches_textbook_example() {
        let table = vec![
            vec![0, 0, 0, 0, 14],
            vec![0, 2, 6, 4, 2],
            vec![0, 0, 3, 5, 6],
            vec![0, 3, 9, 2, 0],
            vec![2, 2, 8, 1, 1],
            vec![7, 7, 0, 0, 0],
            vec![3, 2, 6, 3, 0],
            vec![2, 5, 3, 2, 2],
            vec![6, 5, 2, 1, 0],
            vec![0, 2, 2, 3, 7],
        ];
        close(fleiss_kappa(&table), 0.20993);
    }

    /// Wikipedia「Cohen's kappa」條目的例子：50 份申請，兩人都同意 20、都不同意 15、意見分歧 5 和 10，κ = 0.4。
    #[test]
    fn cohen_matches_textbook_example() {
        let mut pairs = vec![(0, 0); 20];
        pairs.extend(vec![(0, 1); 5]);
        pairs.extend(vec![(1, 0); 10]);
        pairs.extend(vec![(1, 1); 15]);
        close(cohen_kappa(&pairs, 2), 0.4);
        close(percent_agreement(&pairs), 0.7);
    }

    /// Krippendorff (2011)《Computing Krippendorff's Alpha-Reliability》的例子：4 位觀察者、12 筆資料、有缺值。
    /// interval alpha = 0.849。第 12 筆只有一個值，不列入計算。
    #[test]
    fn krippendorff_matches_paper_example() {
        let units: Vec<Vec<f64>> = vec![
            vec![1., 1., 1.],
            vec![2., 2., 3., 2.],
            vec![3., 3., 3., 3.],
            vec![3., 3., 3., 3.],
            vec![2., 2., 2., 2.],
            vec![1., 2., 3., 4.],
            vec![4., 4., 4., 4.],
            vec![1., 1., 2., 1.],
            vec![2., 2., 2., 2.],
            vec![5., 5., 5.],
            vec![1., 1.],
            vec![3.],
        ];
        close(krippendorff_alpha_interval(&units), 0.849);
    }

    #[test]
    fn perfect_and_undefined_agreement() {
        close(fleiss_kappa(&[vec![2, 0], vec![0, 2]]), 1.0);
        // 每個人都選同一個類別：碰巧一致的機率是 1，沒有定義
        assert_eq!(fleiss_kappa(&[vec![3, 0], vec![2, 0]]), None);
        assert_eq!(cohen_kappa(&[(1, 1), (1, 1)], 2), None);
        assert_eq!(krippendorff_alpha_interval(&[vec![2., 2.], vec![2., 2.]]), None);
        // 只標一次的資料不算
        assert_eq!(fleiss_kappa(&[vec![1, 0]]), None);
        assert_eq!(krippendorff_alpha_interval(&[vec![1.]]), None);
    }

    #[test]
    fn fleiss_handles_different_rater_counts() {
        // 第一筆 3 人全同意、第二筆 2 人意見不同
        let k = fleiss_kappa(&[vec![3, 0], vec![1, 1]]).unwrap();
        // P̄ = (1 + 0) / 2 = 0.5；p = (4/5, 1/5)；Pe = 0.68；κ = (0.5 − 0.68) / 0.32
        assert!((k - (0.5 - 0.68) / 0.32).abs() < 1e-9);
    }
}
