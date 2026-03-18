#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BetType {
    Normal,
    FreeBetSnr,
    FreeBetSr,
    RiskFree,
}

impl BetType {
    pub const ALL: [Self; 4] = [
        Self::Normal,
        Self::FreeBetSnr,
        Self::FreeBetSr,
        Self::RiskFree,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::FreeBetSnr => "Free Bet (SNR)",
            Self::FreeBetSr => "Free Bet (SR)",
            Self::RiskFree => "Risk Free",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Simple,
    Advanced,
}

impl Mode {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Simple => Self::Advanced,
            Self::Advanced => Self::Simple,
        };
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "Simple",
            Self::Advanced => "Advanced",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackCommissionMode {
    Profit,
    Return,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartLay {
    pub stake: f64,
    pub odds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub bet_type: BetType,
    pub mode: Mode,
    pub back_stake: f64,
    pub back_odds: f64,
    pub lay_odds: f64,
    pub back_commission_pct: f64,
    pub lay_commission_pct: f64,
    pub back_commission_mode: BackCommissionMode,
    pub risk_free_award: f64,
    pub risk_free_retention_pct: f64,
    pub part_lays: Vec<PartLay>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            bet_type: BetType::Normal,
            mode: Mode::Simple,
            back_stake: 10.0,
            back_odds: 2.55,
            lay_odds: 2.66,
            back_commission_pct: 0.0,
            lay_commission_pct: 0.0,
            back_commission_mode: BackCommissionMode::Profit,
            risk_free_award: 10.0,
            risk_free_retention_pct: 80.0,
            part_lays: vec![
                PartLay {
                    stake: 0.0,
                    odds: 0.0,
                },
                PartLay {
                    stake: 0.0,
                    odds: 0.0,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub lay_stake: f64,
    pub liability: f64,
    pub profit_if_back_wins: f64,
    pub profit_if_lay_wins: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub rating_pct: f64,
    pub qualifying_profit: f64,
    pub retained_risk_free_value: f64,
    pub standard: Scenario,
    pub underlay: Scenario,
    pub overlay: Scenario,
}

pub fn calculate(input: &Input) -> Result<Output, String> {
    validate(input)?;

    let back_stake = input.back_stake;
    let back_odds = input.back_odds;
    let lay_odds = input.lay_odds;
    let lay_comm = input.lay_commission_pct;
    let back_comm = input.back_commission_pct;

    let part_lay_liability: f64 = input
        .part_lays
        .iter()
        .map(|part_lay| part_lay.stake * (part_lay.odds - 1.0))
        .sum();
    let part_lay_stake_total: f64 = input.part_lays.iter().map(|part_lay| part_lay.stake).sum();

    let sr_component = match input.bet_type {
        BetType::FreeBetSnr => 0.0,
        _ => back_stake,
    };
    let risk_free_value = match input.bet_type {
        BetType::RiskFree => input.risk_free_award * input.risk_free_retention_pct / 100.0,
        _ => 0.0,
    };

    let part_lay_return_deduction: f64 = input
        .part_lays
        .iter()
        .map(|part_lay| part_lay.stake * (part_lay.odds - lay_comm / 100.0))
        .sum();

    let back_return = match input.back_commission_mode {
        BackCommissionMode::Profit => {
            ((back_odds - 1.0) * back_stake * (1.0 - back_comm / 100.0)) + sr_component
        }
        BackCommissionMode::Return => {
            ((back_odds - 1.0) * back_stake - (back_stake * back_odds * back_comm / 100.0))
                + sr_component
        }
    };
    let back_return_with_part_lays = back_return - part_lay_return_deduction - risk_free_value;
    let back_return_pure = back_return - risk_free_value;

    let standard_lay = round2(back_return_with_part_lays / (lay_odds - lay_comm / 100.0));
    let standard_liability = standard_lay * (lay_odds - 1.0);
    let standard_profit_win =
        (back_return_pure - standard_liability - part_lay_liability) - back_stake;
    let standard_profit_lose =
        ((standard_lay + part_lay_stake_total) * (100.0 - lay_comm) / 100.0) - back_stake;

    let underlay = if standard_profit_win < 0.0 {
        let liability = back_return_pure - part_lay_liability - back_stake;
        let lay_stake = round2(liability / (lay_odds - 1.0));
        Scenario {
            lay_stake,
            liability,
            profit_if_back_wins: back_return_pure - back_stake - liability - part_lay_liability,
            profit_if_lay_wins: (part_lay_stake_total + lay_stake) * (1.0 - lay_comm / 100.0)
                - back_stake,
        }
    } else {
        let lay_stake = round2(back_stake / (1.0 - lay_comm / 100.0) - part_lay_stake_total);
        let liability = lay_stake * (lay_odds - 1.0);
        Scenario {
            lay_stake,
            liability,
            profit_if_back_wins: back_return_pure - back_stake - liability - part_lay_liability,
            profit_if_lay_wins: (part_lay_stake_total + lay_stake) * (1.0 - lay_comm / 100.0)
                - back_stake,
        }
    };

    let overlay = if standard_profit_win < 0.0 {
        let lay_stake = round2(back_stake / (1.0 - lay_comm / 100.0) - part_lay_stake_total);
        let liability = lay_stake * (lay_odds - 1.0);
        Scenario {
            lay_stake,
            liability,
            profit_if_back_wins: back_return_pure - back_stake - liability - part_lay_liability,
            profit_if_lay_wins: (part_lay_stake_total + lay_stake) * (1.0 - lay_comm / 100.0)
                - back_stake,
        }
    } else {
        let liability = back_return_pure - part_lay_liability - back_stake;
        let lay_stake = round2(liability / (lay_odds - 1.0));
        Scenario {
            lay_stake,
            liability,
            profit_if_back_wins: back_return_pure - back_stake - liability - part_lay_liability,
            profit_if_lay_wins: (part_lay_stake_total + lay_stake) * (1.0 - lay_comm / 100.0)
                - back_stake,
        }
    };

    let standard = Scenario {
        lay_stake: standard_lay,
        liability: standard_liability,
        profit_if_back_wins: standard_profit_win,
        profit_if_lay_wins: standard_profit_lose,
    };

    let standard = adjust_by_bet_type(input.bet_type, risk_free_value, back_stake, standard);
    let underlay = adjust_by_bet_type(input.bet_type, risk_free_value, back_stake, underlay);
    let overlay = adjust_by_bet_type(input.bet_type, risk_free_value, back_stake, overlay);

    let qualifying_profit = standard
        .profit_if_back_wins
        .min(standard.profit_if_lay_wins);
    let rating_pct = if input.bet_type == BetType::Normal {
        (back_stake + qualifying_profit) * 100.0 / back_stake
    } else {
        qualifying_profit * 100.0 / back_stake
    };

    Ok(Output {
        rating_pct: round2(rating_pct),
        qualifying_profit: round2(qualifying_profit),
        retained_risk_free_value: round2(risk_free_value),
        standard: round_scenario(standard),
        underlay: round_scenario(underlay),
        overlay: round_scenario(overlay),
    })
}

fn validate(input: &Input) -> Result<(), String> {
    if input.back_stake <= 0.0 {
        return Err(String::from("Back stake must be greater than zero."));
    }
    if input.back_odds <= 1.0 {
        return Err(String::from("Back odds must be greater than 1.0."));
    }
    if input.lay_odds <= 1.0 {
        return Err(String::from("Lay odds must be greater than 1.0."));
    }
    if !(0.0..100.0).contains(&input.lay_commission_pct) {
        return Err(String::from("Lay commission must be between 0 and 100."));
    }
    if !(0.0..100.0).contains(&input.back_commission_pct) {
        return Err(String::from("Bookie commission must be between 0 and 100."));
    }
    if !(0.0..=100.0).contains(&input.risk_free_retention_pct) {
        return Err(String::from(
            "Risk-free retention must be between 0 and 100.",
        ));
    }
    if input
        .part_lays
        .iter()
        .any(|part_lay| part_lay.stake < 0.0 || part_lay.odds < 0.0)
    {
        return Err(String::from("Part-lay values cannot be negative."));
    }
    Ok(())
}

fn adjust_by_bet_type(
    bet_type: BetType,
    risk_free_value: f64,
    back_stake: f64,
    scenario: Scenario,
) -> Scenario {
    match bet_type {
        BetType::Normal => scenario,
        BetType::FreeBetSr | BetType::FreeBetSnr => Scenario {
            profit_if_back_wins: scenario.profit_if_back_wins + back_stake,
            profit_if_lay_wins: scenario.profit_if_lay_wins + back_stake,
            ..scenario
        },
        BetType::RiskFree => Scenario {
            profit_if_back_wins: scenario.profit_if_back_wins + risk_free_value,
            profit_if_lay_wins: scenario.profit_if_lay_wins + risk_free_value,
            ..scenario
        },
    }
}

fn round_scenario(scenario: Scenario) -> Scenario {
    Scenario {
        lay_stake: round2(scenario.lay_stake),
        liability: round2(scenario.liability),
        profit_if_back_wins: round2(scenario.profit_if_back_wins),
        profit_if_lay_wins: round2(scenario.profit_if_lay_wins),
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::{calculate, BetType, Input, Mode};

    #[test]
    fn normal_mode_matches_captured_oddsmatcher_example() {
        let input = Input {
            bet_type: BetType::Normal,
            mode: Mode::Simple,
            back_stake: 10.0,
            back_odds: 2.55,
            lay_odds: 2.66,
            back_commission_pct: 0.0,
            lay_commission_pct: 0.0,
            ..Input::default()
        };

        let output = calculate(&input).expect("calculation should succeed");

        assert_eq!(output.standard.lay_stake, 9.59);
        assert_eq!(output.standard.liability, 15.92);
        assert_eq!(output.qualifying_profit, -0.42);
        assert_eq!(output.rating_pct, 95.81);
    }

    #[test]
    fn snr_mode_uses_free_bet_style_profit_rating() {
        let input = Input {
            bet_type: BetType::FreeBetSnr,
            back_stake: 10.0,
            back_odds: 5.4,
            lay_odds: 5.5,
            lay_commission_pct: 2.0,
            ..Input::default()
        };

        let output = calculate(&input).expect("calculation should succeed");

        assert!(output.standard.profit_if_back_wins > 0.0);
        assert!(output.standard.profit_if_lay_wins > 0.0);
        assert!(output.rating_pct > 70.0);
    }

    #[test]
    fn risk_free_mode_adds_retained_value_to_both_outcomes() {
        let input = Input {
            bet_type: BetType::RiskFree,
            back_stake: 25.0,
            back_odds: 3.0,
            lay_odds: 3.1,
            lay_commission_pct: 2.0,
            risk_free_award: 25.0,
            risk_free_retention_pct: 80.0,
            ..Input::default()
        };

        let output = calculate(&input).expect("calculation should succeed");

        assert_eq!(output.retained_risk_free_value, 20.0);
        assert!(output.standard.profit_if_back_wins > -5.0);
        assert!(output.standard.profit_if_lay_wins > -5.0);
    }

    #[test]
    fn invalid_odds_are_rejected() {
        let input = Input {
            back_odds: 1.0,
            ..Input::default()
        };

        let error = calculate(&input).expect_err("invalid input should fail");
        assert!(error.contains("Back odds"));
    }
}
