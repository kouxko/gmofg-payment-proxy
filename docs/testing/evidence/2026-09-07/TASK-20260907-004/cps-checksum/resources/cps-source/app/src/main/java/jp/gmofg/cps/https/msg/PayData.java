/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20231010 	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

public class PayData extends BaseData{

    private PayResult result;

    public void setResult(PayResult result) {
        this.result = result;
    }

    @Override
    public PayResult getBaseResult() {
        return result;
    }

    public static class PayResult extends BaseResult {

        private String currencyCode;
        private int amount;
        private int amountRmb;//not used
        private String payType;
        private String extendedInfo;

        public void setCurrencyCode(String currencyCode) {
            this.currencyCode = currencyCode;
        }
        public String getCurrencyCode() {
            return currencyCode;
        }

        public void setAmount(int amount) {
            this.amount = amount;
        }
        public int getAmount() {
            return amount;
        }

        public int getAmountRmb() {
            return amountRmb;
        }
        public void setAmountRmb(int amountRmb) {
            this.amountRmb = amountRmb;
        }

        public void setPayType(String payType) {
            this.payType = payType;
        }
        public String getPayType() {
            return payType;
        }

        public String getExtendedInfo() {
            return extendedInfo;
        }
        public void setExtendedInfo(String extendedInfo) {
            this.extendedInfo = extendedInfo;
        }
    }

}
